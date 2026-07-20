use crate::app::AppContext;
use crate::cli::UpdateArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

use std::time::Duration;

const GITHUB_RELEASES_URL: &str = "https://github.com/ctykwz/sunox/releases/latest";
const GITHUB_RELEASE_DOWNLOAD_URL: &str = "https://github.com/ctykwz/sunox/releases/download";
const MAX_RELEASE_ASSET_BYTES: usize = 64 * 1024 * 1024;
const MAX_EXTRACTED_BINARY_BYTES: u64 = 128 * 1024 * 1024;
const RELEASE_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RELEASE_API_TIMEOUT: Duration = Duration::from_secs(30);
const RELEASE_REQUEST_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const RELEASE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn run(args: UpdateArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _proxy_guard = crate::net::proxy::UpdateProxyEnvGuard::activate();
    let current = env!("CARGO_PKG_VERSION");
    let updater = build_updater(current, !ctx.quiet, |_| {})?;

    if args.check {
        let releases = match updater.get_latest_release() {
            Ok(releases) => releases,
            Err(api_error) => return run_web_fallback(args, ctx, current, api_error).await,
        };
        let latest = releases
            .latest()
            .ok_or_else(|| CliError::Update("GitHub returned no releases".into()))?;
        let v = latest.version().trim_start_matches('v').to_string();
        let available = update_available(current, &v)?;
        let status = if available {
            "update_available"
        } else {
            "up_to_date"
        };
        let result = serde_json::json!({
            "current_version": current,
            "latest_version": v,
            "status": status,
        });
        match ctx.fmt {
            OutputFormat::Json => output::json::success(&result),
            OutputFormat::Table => {
                if available {
                    eprintln!("Update available: v{current} -> v{v}");
                    eprintln!("Run `sunox update` to install");
                } else {
                    eprintln!("Up to date (v{current})");
                }
            }
        }
    } else {
        let releases = match updater.get_latest_release() {
            Ok(releases) => releases,
            Err(api_error) => return run_web_fallback(args, ctx, current, api_error).await,
        };
        let latest = releases
            .latest()
            .ok_or_else(|| CliError::Update("GitHub returned no releases".into()))?;
        let latest_version = latest.version().trim_start_matches('v');
        if !update_available(current, latest_version)? {
            output_update_result(current, latest_version, true, ctx);
            return Ok(());
        }

        let release_tag = if latest.version().starts_with('v') {
            latest.version().to_string()
        } else {
            format!("v{}", latest.version())
        };
        ensure_install_directory_writable()?;
        let client = build_release_client()?;
        install_web_release(&client, GITHUB_RELEASE_DOWNLOAD_URL, &release_tag).await?;
        output_update_result(current, latest_version, false, ctx);
    }

    Ok(())
}

async fn run_web_fallback(
    args: UpdateArgs,
    ctx: &AppContext,
    current: &str,
    api_error: self_update::errors::Error,
) -> Result<(), CliError> {
    let client = build_release_client()?;
    let tag = latest_release_tag(&client, GITHUB_RELEASES_URL, Some("github.com"))
        .await
        .map_err(|web_error| {
            CliError::Update(format!(
                "GitHub API failed ({api_error}); web fallback failed ({web_error})"
            ))
        })?;
    let latest = tag.strip_prefix('v').unwrap_or(&tag);
    let available = update_available(current, latest)?;

    if args.check {
        let status = if available {
            "update_available"
        } else {
            "up_to_date"
        };
        let result = serde_json::json!({
            "current_version": current,
            "latest_version": latest,
            "status": status,
        });
        match ctx.fmt {
            OutputFormat::Json => output::json::success(&result),
            OutputFormat::Table if available => {
                eprintln!("Update available: v{current} -> v{latest}");
                eprintln!("Run `sunox update` to install");
            }
            OutputFormat::Table => eprintln!("Up to date (v{current})"),
        }
        return Ok(());
    }
    if !available {
        output_update_result(current, latest, true, ctx);
        return Ok(());
    }

    ensure_install_directory_writable()?;
    install_web_release(&client, GITHUB_RELEASE_DOWNLOAD_URL, &tag).await?;
    output_update_result(current, latest, false, ctx);
    Ok(())
}

fn build_release_client() -> Result<reqwest::Client, CliError> {
    reqwest::Client::builder()
        .user_agent(concat!("sunox/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(RELEASE_CONNECT_TIMEOUT)
        .timeout(RELEASE_REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("too many redirects")
            } else if attempt.url().scheme() != "https" {
                attempt.error("release download redirected to an insecure URL")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|error| CliError::Update(error.to_string()))
}

async fn latest_release_tag(
    client: &reqwest::Client,
    latest_url: &str,
    expected_host: Option<&str>,
) -> Result<String, CliError> {
    let response = client
        .get(latest_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| CliError::Update(error.to_string()))?;
    release_tag_from_url(response.url(), expected_host)
}

async fn install_web_release(
    client: &reqwest::Client,
    download_base: &str,
    tag: &str,
) -> Result<(), CliError> {
    let target = self_update::get_target();
    let staging = tempfile::Builder::new()
        .prefix("sunox-update-")
        .tempdir()
        .map_err(|error| CliError::Update(error.to_string()))?;
    let binary =
        download_and_extract_web_release(client, download_base, tag, target, staging.path())
            .await?;
    replace_current_executable(&binary)?;
    Ok(())
}

async fn download_and_extract_web_release(
    client: &reqwest::Client,
    download_base: &str,
    tag: &str,
    target: &str,
    staging: &std::path::Path,
) -> Result<std::path::PathBuf, CliError> {
    let asset_name = release_asset_name(target);
    let base = format!("{download_base}/{tag}");
    let sums = download_limited(client, &format!("{base}/SHA256SUMS"), 1024 * 1024).await?;
    let sums = String::from_utf8(sums)
        .map_err(|_| CliError::Update("SHA256SUMS is not valid UTF-8".into()))?;
    let expected = release_checksum_for_asset(&sums, &asset_name)?;
    let archive = download_limited(
        client,
        &format!("{base}/{asset_name}"),
        MAX_RELEASE_ASSET_BYTES,
    )
    .await?;
    verify_release_checksum(&archive, &expected)?;

    let archive_path = staging.join(&asset_name);
    std::fs::write(&archive_path, archive).map_err(|error| CliError::Update(error.to_string()))?;
    let binary_name = if target.contains("windows") {
        "sunox.exe"
    } else {
        "sunox"
    };
    validate_extracted_binary_size(&archive_path, binary_name)?;
    self_update::Extract::from_source(&archive_path)
        .extract_file(staging, binary_name)
        .map_err(|error| CliError::Update(error.to_string()))?;
    Ok(staging.join(binary_name))
}

async fn download_limited(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, CliError> {
    download_limited_with_idle_timeout(client, url, limit, RELEASE_IDLE_TIMEOUT).await
}

async fn download_limited_with_idle_timeout(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
    idle_timeout: Duration,
) -> Result<Vec<u8>, CliError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| CliError::Update(error.to_string()))?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(CliError::Update(format!(
            "release download exceeded {limit} bytes"
        )));
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = tokio::time::timeout(idle_timeout, response.chunk())
            .await
            .map_err(|_| {
                CliError::Update(format!(
                    "release download stalled for {} seconds",
                    idle_timeout.as_secs_f32()
                ))
            })?
            .map_err(|error| CliError::Update(error.to_string()))?;
        let Some(chunk) = chunk else {
            break;
        };
        let remaining = limit.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            return Err(CliError::Update(format!(
                "release download exceeded {limit} bytes"
            )));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(not(windows))]
fn validate_extracted_binary_size(
    archive_path: &std::path::Path,
    binary_name: &str,
) -> Result<(), CliError> {
    let source =
        std::fs::File::open(archive_path).map_err(|error| CliError::Update(error.to_string()))?;
    let decoder = flate2::read::GzDecoder::new(source);
    let mut archive = tar::Archive::new(decoder);
    let entry = archive
        .entries()
        .map_err(|error| CliError::Update(error.to_string()))?
        .next()
        .ok_or_else(|| CliError::Update("release archive is empty".into()))?
        .map_err(|error| CliError::Update(error.to_string()))?;
    let path = entry
        .path()
        .map_err(|error| CliError::Update(error.to_string()))?;
    if path != std::path::Path::new(binary_name) || !entry.header().entry_type().is_file() {
        return Err(CliError::Update(format!(
            "release archive must contain {binary_name} as its first regular entry"
        )));
    }
    validate_binary_size(entry.size(), binary_name)
}

#[cfg(windows)]
fn validate_extracted_binary_size(
    archive_path: &std::path::Path,
    binary_name: &str,
) -> Result<(), CliError> {
    let source =
        std::fs::File::open(archive_path).map_err(|error| CliError::Update(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(source).map_err(|error| CliError::Update(error.to_string()))?;
    let entry = archive
        .by_name(binary_name)
        .map_err(|error| CliError::Update(error.to_string()))?;
    validate_binary_size(entry.size(), binary_name)
}

fn validate_binary_size(size: u64, binary_name: &str) -> Result<(), CliError> {
    if size > MAX_EXTRACTED_BINARY_BYTES {
        return Err(CliError::Update(format!(
            "release binary {binary_name} exceeded {MAX_EXTRACTED_BINARY_BYTES} bytes after decompression"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_current_executable(new_executable: &std::path::Path) -> Result<(), CliError> {
    self_replace::self_replace(new_executable).map_err(|error| CliError::Update(error.to_string()))
}

#[cfg(windows)]
fn replace_current_executable(new_executable: &std::path::Path) -> Result<(), CliError> {
    let current = std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map_err(|error| CliError::Update(error.to_string()))?;
    let parent = current
        .parent()
        .ok_or_else(|| CliError::Update("current executable has no parent directory".into()))?;
    let nonce = uuid::Uuid::new_v4();
    let staged = parent.join(format!(".sunox-update-{nonce}.exe"));
    let backup = parent.join(format!(".sunox-backup-{nonce}.exe"));
    std::fs::copy(new_executable, &staged).map_err(|error| CliError::Update(error.to_string()))?;
    let replace_result = replace_file_with_rollback(&current, &staged, &backup);
    if replace_result.is_err() {
        let _ = std::fs::remove_file(&staged);
    }
    replace_result?;
    if let Err(error) = self_replace::self_delete_at(&backup) {
        eprintln!(
            "Warning: updated successfully but could not schedule old executable cleanup ({}): {error}",
            backup.display()
        );
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn replace_file_with_rollback(
    current: &std::path::Path,
    staged: &std::path::Path,
    backup: &std::path::Path,
) -> Result<(), CliError> {
    std::fs::rename(current, backup).map_err(|error| CliError::Update(error.to_string()))?;
    if let Err(install_error) = std::fs::rename(staged, current) {
        return match std::fs::rename(backup, current) {
            Ok(()) => Err(CliError::Update(format!(
                "failed to install new executable; restored previous version: {install_error}"
            ))),
            Err(rollback_error) => Err(CliError::Update(format!(
                "failed to install new executable ({install_error}) and failed to restore previous version from {} ({rollback_error})",
                backup.display()
            ))),
        };
    }
    Ok(())
}

fn release_tag_from_url(
    url: &reqwest::Url,
    expected_host: Option<&str>,
) -> Result<String, CliError> {
    if expected_host.is_some_and(|host| url.host_str() != Some(host)) {
        return Err(CliError::Update(format!(
            "GitHub latest release redirected to unexpected host: {url}"
        )));
    }
    if expected_host.is_some() && url.scheme() != "https" {
        return Err(CliError::Update(format!(
            "GitHub latest release redirected to an insecure URL: {url}"
        )));
    }
    let segments = url
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();
    let ["ctykwz", "sunox", "releases", "tag", tag] = segments.as_slice() else {
        return Err(CliError::Update(format!(
            "GitHub latest release redirected to unexpected URL: {url}"
        )));
    };
    let version = tag.strip_prefix('v').unwrap_or(tag);
    if tag.is_empty()
        || tag.contains('/')
        || self_update::version::bump_is_greater("0.0.0", version).is_err()
    {
        return Err(CliError::Update(format!(
            "GitHub latest release returned an invalid tag: {tag}"
        )));
    }
    Ok(tag.to_string())
}

fn release_asset_name(target: &str) -> String {
    let extension = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("sunox-{target}.{extension}")
}

fn verify_release_checksum(bytes: &[u8], expected: &str) -> Result<(), CliError> {
    use sha2::Digest;

    let actual = sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != expected {
        return Err(CliError::Update(format!(
            "release asset checksum mismatch: expected {expected}, got {actual}"
        )));
    }
    Ok(())
}

fn ensure_install_directory_writable() -> Result<(), CliError> {
    let executable = std::env::current_exe()
        .map_err(|error| CliError::Update(format!("cannot locate current executable: {error}")))?;
    let install_dir = executable.parent().ok_or_else(|| {
        CliError::Update("current executable has no parent installation directory".into())
    })?;
    tempfile::Builder::new()
        .prefix(".sunox-update-write-test-")
        .tempfile_in(install_dir)
        .map_err(|error| {
            CliError::Update(format!(
                "installation directory is not writable ({}): {error}. Re-run from an elevated terminal or install sunox in a user-writable directory",
                install_dir.display()
            ))
        })?;
    Ok(())
}

fn output_update_result(current: &str, latest: &str, up_to_date: bool, ctx: &AppContext) {
    let status = if up_to_date { "up_to_date" } else { "updated" };
    let result = serde_json::json!({
        "current_version": current,
        "latest_version": latest,
        "status": status,
    });
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&result),
        OutputFormat::Table => {
            if up_to_date {
                eprintln!("Already up to date (v{current})");
            } else {
                eprintln!("Updated: v{current} -> v{latest}");
                eprintln!("Run `sunox install-skill --force` to refresh the agent skill");
            }
        }
    }
}

fn release_checksum_for_asset(sums: &str, asset_name: &str) -> Result<String, CliError> {
    for line in sums.lines() {
        let mut fields = line.split_whitespace();
        let Some(digest) = fields.next() else {
            continue;
        };
        let Some(name) = fields.next() else {
            continue;
        };
        if name.trim_start_matches('*') == asset_name
            && digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Ok(digest.to_ascii_lowercase());
        }
    }
    Err(CliError::Update(format!(
        "SHA256SUMS has no valid entry for {asset_name}"
    )))
}

fn build_updater(
    current: &str,
    show_download_progress: bool,
    configure: impl FnOnce(&mut self_update::backends::github::UpdateBuilder),
) -> Result<self_update::backends::github::Update, CliError> {
    let mut builder = self_update::backends::github::Update::configure();
    builder
        .repo_owner("ctykwz")
        .repo_name("sunox")
        .bin_name("sunox")
        .timeout(RELEASE_API_TIMEOUT)
        .show_download_progress(show_download_progress)
        .no_confirm(true)
        .current_version(current);
    if let Some(token) = ["GITHUB_TOKEN", "GH_TOKEN"].into_iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }) {
        builder.auth_token(token);
    }
    configure(&mut builder);
    builder
        .build()
        .map_err(|error| CliError::Update(error.to_string()))
}

fn update_available(current: &str, latest: &str) -> Result<bool, CliError> {
    self_update::version::bump_is_greater(current, latest)
        .map_err(|error| CliError::Update(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::path::Path;
    use std::time::Duration;

    use sha2::{Digest, Sha256};

    use super::{
        MAX_EXTRACTED_BINARY_BYTES, download_and_extract_web_release, download_limited,
        download_limited_with_idle_timeout, latest_release_tag, release_asset_name,
        release_checksum_for_asset, release_tag_from_url, replace_file_with_rollback,
        update_available, validate_binary_size, validate_extracted_binary_size,
        verify_release_checksum,
    };

    #[test]
    fn release_checksum_selects_the_exact_platform_asset() {
        let sums = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  sunox-x86_64-unknown-linux-gnu.tar.gz\n\
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  sunox-x86_64-pc-windows-msvc.zip\n";

        let checksum = release_checksum_for_asset(sums, "sunox-x86_64-pc-windows-msvc.zip")
            .expect("platform checksum");

        assert_eq!(
            checksum,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn release_checksum_rejects_a_missing_platform_asset() {
        let error = release_checksum_for_asset(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  other.tar.gz\n",
            "sunox-x86_64-unknown-linux-gnu.tar.gz",
        )
        .expect_err("missing checksum must abort update");

        assert!(error.to_string().contains("SHA256SUMS"));
    }

    #[test]
    fn update_is_available_only_when_the_release_is_newer() {
        assert!(update_available("0.0.13", "0.0.14").expect("valid versions"));
        assert!(!update_available("0.0.13", "0.0.13").expect("valid versions"));
        assert!(!update_available("0.0.14", "0.0.13").expect("valid versions"));
    }

    #[test]
    fn web_fallback_accepts_only_expected_release_urls() {
        let url = reqwest::Url::parse("https://github.com/ctykwz/sunox/releases/tag/v0.0.24")
            .expect("release URL");
        assert_eq!(
            release_tag_from_url(&url, Some("github.com")).expect("release tag"),
            "v0.0.24"
        );

        let wrong_host =
            reqwest::Url::parse("https://example.com/releases/tag/v0.0.24").expect("URL");
        assert!(release_tag_from_url(&wrong_host, Some("github.com")).is_err());

        let invalid_version =
            reqwest::Url::parse("https://github.com/ctykwz/sunox/releases/tag/not-a-version")
                .expect("URL");
        assert!(release_tag_from_url(&invalid_version, Some("github.com")).is_err());

        let insecure = reqwest::Url::parse("http://github.com/ctykwz/sunox/releases/tag/v0.0.24")
            .expect("URL");
        assert!(release_tag_from_url(&insecure, Some("github.com")).is_err());

        let wrong_repository =
            reqwest::Url::parse("https://github.com/other/sunox/releases/tag/v0.0.24")
                .expect("URL");
        assert!(release_tag_from_url(&wrong_repository, Some("github.com")).is_err());

        let repeated_prefix =
            reqwest::Url::parse("https://github.com/ctykwz/sunox/releases/tag/vv0.0.24")
                .expect("URL");
        assert!(release_tag_from_url(&repeated_prefix, Some("github.com")).is_err());
    }

    #[test]
    fn web_fallback_uses_published_asset_names() {
        assert_eq!(
            release_asset_name("aarch64-apple-darwin"),
            "sunox-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            release_asset_name("x86_64-pc-windows-msvc"),
            "sunox-x86_64-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn web_fallback_rejects_checksum_mismatch() {
        let expected = Sha256::digest(b"expected")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert!(verify_release_checksum(b"tampered", &expected).is_err());
    }

    #[test]
    fn web_fallback_rejects_oversized_extracted_binary() {
        let error = validate_binary_size(MAX_EXTRACTED_BINARY_BYTES + 1, "sunox")
            .expect_err("oversized binary must fail");
        assert!(error.to_string().contains("after decompression"));
    }

    #[test]
    fn replacement_failure_restores_previous_binary() {
        let temp = tempfile::tempdir().expect("temp dir");
        let current = temp.path().join("sunox");
        let missing_staged = temp.path().join("missing-new-sunox");
        let backup = temp.path().join("sunox.backup");
        std::fs::write(&current, b"old binary").expect("seed old binary");

        let error = replace_file_with_rollback(&current, &missing_staged, &backup)
            .expect_err("missing staged binary must fail");

        assert!(error.to_string().contains("restored previous version"));
        assert_eq!(
            std::fs::read(&current).expect("restored binary"),
            b"old binary"
        );
        assert!(!backup.exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn web_fallback_rejects_an_entry_before_the_binary() {
        let archive = release_archive_with_leading_entry();
        let temp = tempfile::tempdir().expect("temp dir");
        let archive_path = temp.path().join("release.tar.gz");
        std::fs::write(&archive_path, archive).expect("write archive");

        let error = validate_extracted_binary_size(&archive_path, "sunox")
            .expect_err("leading archive entry must fail");

        assert!(error.to_string().contains("first regular entry"));
    }

    #[tokio::test]
    async fn web_fallback_follows_latest_release_redirect() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("release listener");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        );
        let redirect = format!("{base_url}/ctykwz/sunox/releases/tag/v0.0.24");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("redirect request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read redirect request");
            write!(
                stream,
                "HTTP/1.1 302 Found\r\nLocation: {redirect}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write redirect");

            let (mut stream, _) = listener.accept().expect("release page request");
            let _ = stream
                .read(&mut request)
                .expect("read release page request");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            )
            .expect("write release page");
        });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(2))
            .build()
            .expect("client");

        let tag = latest_release_tag(&client, &format!("{base_url}/releases/latest"), None)
            .await
            .expect("latest tag");
        server.join().expect("release server");

        assert_eq!(tag, "v0.0.24");
    }

    #[tokio::test]
    async fn web_fallback_streaming_limit_rejects_oversized_body() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("download listener");
        let url = format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("download request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read download request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n12345")
                .expect("write oversized response");
        });

        let error = download_limited(&reqwest::Client::new(), &url, 4)
            .await
            .expect_err("oversized response must fail");
        server.join().expect("download server");

        assert!(error.to_string().contains("exceeded 4 bytes"));
    }

    #[tokio::test]
    async fn web_fallback_rejects_a_stalled_download() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("download listener");
        let url = format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("download request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read download request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
                .expect("write response headers");
            std::thread::sleep(Duration::from_millis(100));
        });

        let error = download_limited_with_idle_timeout(
            &reqwest::Client::new(),
            &url,
            4,
            Duration::from_millis(10),
        )
        .await
        .expect_err("stalled response must fail");
        server.join().expect("download server");

        assert!(error.to_string().contains("stalled"));
    }

    #[tokio::test]
    async fn web_fallback_downloads_verifies_and_extracts_release() {
        let payload = b"sunox web fallback fixture";
        let archive = release_archive(payload);
        let checksum = Sha256::digest(&archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let target = self_update::get_target();
        let asset_name = release_asset_name(target);
        let sums = format!("{checksum}  {asset_name}\n").into_bytes();
        let (download_base, server) = serve_web_release(sums, archive);
        let staging = tempfile::tempdir().expect("staging directory");

        let binary = download_and_extract_web_release(
            &reqwest::Client::new(),
            &download_base,
            "v0.0.24",
            target,
            staging.path(),
        )
        .await
        .expect("download release");
        server.join().expect("release server");

        assert_eq!(std::fs::read(binary).expect("extracted binary"), payload);
    }

    #[test]
    fn updater_downloads_extracts_and_replaces_the_target_binary() {
        let payload = b"sunox replacement fixture";
        let archive = release_archive(payload);
        let checksum = Sha256::digest(&archive)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let target = self_update::get_target();
        let asset_name = format!("sunox-{target}.{}", release_archive_extension());
        let (api_base_url, server) = serve_release(&asset_name, archive);
        let temp = tempfile::tempdir().expect("temp dir");
        let install_path = temp.path().join(installed_binary_name());
        std::fs::write(&install_path, b"old binary").expect("seed target binary");

        let updater = super::build_updater("0.0.13", false, |builder| {
            builder
                .api_base_url(&api_base_url)
                .bin_install_path(&install_path)
                .show_output(false)
                .target(target)
                .verify_checksum(self_update::Checksum::Sha256(checksum));
        })
        .expect("build updater");

        updater.update().expect("install fixture release");
        server.join().expect("release server");

        assert_eq!(
            std::fs::read(&install_path).expect("installed binary"),
            payload
        );
    }

    #[test]
    fn updater_api_request_has_a_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("API listener");
        let api_base_url = format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        );
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("API request");
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).expect("read API request");
            std::thread::sleep(Duration::from_millis(100));
        });
        let updater = super::build_updater("0.0.23", false, |builder| {
            builder
                .api_base_url(api_base_url)
                .timeout(Duration::from_millis(10));
        })
        .expect("build updater");

        let error = updater
            .get_latest_release()
            .expect_err("stalled API must time out");
        server.join().expect("API server");

        let message = error.to_string().to_ascii_lowercase();
        assert!(
            message.contains("timed out") || message.contains("timeout"),
            "unexpected timeout error: {error}"
        );
    }

    #[test]
    fn updater_selects_each_published_target_asset() {
        let targets = [
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "aarch64-pc-windows-msvc",
        ];
        let assets = targets
            .iter()
            .map(|target| {
                let extension = if target.contains("windows") {
                    "zip"
                } else {
                    "tar.gz"
                };
                let name = format!("sunox-{target}.{extension}");
                self_update::ReleaseAsset::new(name.clone(), format!("https://example/{name}"))
            })
            .collect::<Vec<_>>();
        let mut builder = self_update::Release::builder();
        builder.version("0.0.14").assets(assets);
        let release = builder.build().expect("release fixture");

        for target in targets {
            let selected = release.asset_for(target, None).expect("target asset");
            assert!(selected.name().contains(target));
        }
    }

    fn serve_release(asset_name: &str, archive: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("release listener");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        );
        let release = format!(
            r#"[{{"tag_name":"v0.0.14","created_at":"2026-07-10T00:00:00Z","name":"v0.0.14","assets":[{{"name":"{asset_name}","url":"{base_url}/asset"}}]}}]"#
        );
        let responses = [
            ("application/json", release.into_bytes()),
            ("application/octet-stream", archive),
        ];
        let server = std::thread::spawn(move || {
            for (content_type, body) in responses {
                let (mut stream, _) = listener.accept().expect("release request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read release request");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .expect("write release response headers");
                stream.write_all(&body).expect("write release response");
            }
        });
        (base_url, server)
    }

    fn serve_web_release(sums: Vec<u8>, archive: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("release listener");
        let base_url = format!(
            "http://{}",
            listener.local_addr().expect("listener address")
        );
        let server = std::thread::spawn(move || {
            for body in [sums, archive] {
                let (mut stream, _) = listener.accept().expect("release request");
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request).expect("read release request");
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .expect("write release response headers");
                stream.write_all(&body).expect("write release response");
            }
        });
        (base_url, server)
    }

    #[cfg(not(windows))]
    fn release_archive(payload: &[u8]) -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, "sunox", payload)
            .expect("append binary to tar");
        archive
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip")
    }

    #[cfg(not(windows))]
    fn release_archive_with_leading_entry() -> Vec<u8> {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        for (name, payload) in [("unexpected", &b"x"[..]), ("sunox", &b"binary"[..])] {
            let mut header = tar::Header::new_gnu();
            header.set_size(payload.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive
                .append_data(&mut header, name, payload)
                .expect("append archive entry");
        }
        archive
            .into_inner()
            .expect("finish tar")
            .finish()
            .expect("finish gzip")
    }

    #[cfg(windows)]
    fn release_archive(payload: &[u8]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut archive = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive
            .start_file("sunox.exe", options)
            .expect("start zip binary");
        archive.write_all(payload).expect("write zip binary");
        archive.finish().expect("finish zip").into_inner()
    }

    #[cfg(not(windows))]
    fn release_archive_extension() -> &'static str {
        "tar.gz"
    }

    #[cfg(windows)]
    fn release_archive_extension() -> &'static str {
        "zip"
    }

    fn installed_binary_name() -> &'static Path {
        Path::new(if cfg!(windows) { "sunox.exe" } else { "sunox" })
    }
}
