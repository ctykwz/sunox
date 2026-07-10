use crate::app::AppContext;
use crate::cli::UpdateArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn run(args: UpdateArgs, ctx: &AppContext) -> Result<(), CliError> {
    let current = env!("CARGO_PKG_VERSION");
    let updater = build_updater(current, !ctx.quiet, |_| {})?;

    if args.check {
        let releases = updater
            .get_latest_release()
            .map_err(|e| CliError::Update(e.to_string()))?;
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
        let releases = updater
            .get_latest_release()
            .map_err(|e| CliError::Update(e.to_string()))?;
        let latest = releases
            .latest()
            .ok_or_else(|| CliError::Update("GitHub returned no releases".into()))?;
        let latest_version = latest.version().trim_start_matches('v');
        if !update_available(current, latest_version)? {
            output_update_result(current, latest_version, true, ctx);
            return Ok(());
        }

        let (asset_name, checksum) = release_asset_checksum(latest)?;
        let release_tag = if latest.version().starts_with('v') {
            latest.version().to_string()
        } else {
            format!("v{}", latest.version())
        };
        let verified_updater = build_updater(current, !ctx.quiet, |builder| {
            builder
                .release_tag(release_tag)
                .asset_matcher(move |assets| {
                    assets
                        .iter()
                        .find(|asset| asset.name() == asset_name)
                        .cloned()
                })
                .verify_checksum(self_update::Checksum::Sha256(checksum));
        })?;
        let release = verified_updater
            .update()
            .map_err(|e| CliError::Update(e.to_string()))?;
        let v = release.version().trim_start_matches('v').to_string();
        output_update_result(current, &v, v == current, ctx);
    }

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

fn release_asset_checksum(release: &self_update::Release) -> Result<(String, String), CliError> {
    let target = self_update::get_target();
    let asset = release
        .asset_for(target, None)
        .ok_or_else(|| CliError::Update(format!("release has no asset for target {target}")))?;
    let checksum_asset = release
        .assets()
        .iter()
        .find(|asset| asset.name() == "SHA256SUMS")
        .ok_or_else(|| CliError::Update("release is missing SHA256SUMS".into()))?;
    let mut bytes = Vec::new();
    self_update::Download::from_url(checksum_asset.download_url())
        .request_header(
            self_update::http::header::ACCEPT,
            "application/octet-stream",
        )
        .max_download_size(1024 * 1024)
        .download_to(&mut bytes)
        .map_err(|error| CliError::Update(error.to_string()))?;
    let sums = String::from_utf8(bytes)
        .map_err(|_| CliError::Update("SHA256SUMS is not valid UTF-8".into()))?;
    let checksum = release_checksum_for_asset(&sums, asset.name())?;
    Ok((asset.name().to_string(), checksum))
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
        .show_download_progress(show_download_progress)
        .no_confirm(true)
        .current_version(current);
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

    use sha2::{Digest, Sha256};

    use super::{release_checksum_for_asset, update_available};

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
    fn updater_downloads_extracts_and_replaces_the_target_binary() {
        let payload = b"sunox replacement fixture";
        let archive = release_archive(payload);
        let checksum = format!("{:x}", Sha256::digest(&archive));
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
    fn updater_selects_each_published_target_asset() {
        let targets = [
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "x86_64-unknown-linux-gnu",
            "aarch64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
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
