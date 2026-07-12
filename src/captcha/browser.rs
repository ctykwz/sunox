use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use sysinfo::{ProcessesToUpdate, System};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::sleep;

use super::cdp;
use crate::browser::locate_chromium_browser;
use crate::core::CliError;

pub(super) struct BrowserSession {
    child: Child,
    _profile: TempDir,
    port: u16,
}

impl BrowserSession {
    pub(super) fn port(&self) -> u16 {
        self.port
    }

    pub(super) async fn shutdown(mut self) -> Result<(), CliError> {
        if self.child.try_wait()?.is_none() {
            self.child.kill().await?;
        }
        self.child.wait().await?;
        Ok(())
    }
}

/// Launch a browser owned by this invocation. Chrome chooses an unused CDP
/// port and records it inside the invocation's private temporary profile.
pub(super) async fn launch() -> Result<BrowserSession, CliError> {
    let browser_path = locate_chromium_browser()?;
    let profile = tempfile::Builder::new()
        .prefix("sunox-captcha-")
        .tempdir()
        .map_err(CliError::Io)?;
    let active_port_path = profile.path().join("DevToolsActivePort");

    eprintln!("Launching browser for captcha solver...");

    // Do not use --headless. hCaptcha's bot-detection trips on headless mode.
    let mut command = Command::new(&browser_path);
    command
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.path().display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-features=TranslateUI")
        .arg("--window-position=-32000,-32000")
        .arg("--window-size=1280,900")
        .arg("--silent-launch")
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        CliError::Config(format!(
            "failed to spawn browser at {browser_path:?}: {error}"
        ))
    })?;
    drain_stderr(&mut child);

    for _ in 0..40 {
        if let Some(status) = child.try_wait()? {
            return Err(CliError::Config(format!(
                "captcha browser exited before CDP became ready: {status}"
            )));
        }
        if let Ok(port) = read_owned_cdp_port(&active_port_path)
            && cdp::cdp_version(port).await.is_ok()
        {
            return Ok(BrowserSession {
                child,
                _profile: profile,
                port,
            });
        }
        sleep(Duration::from_millis(250)).await;
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
    Err(CliError::Config(
        "Browser was spawned but never exposed its owned CDP endpoint. Check that Chrome or Edge can start normally, or set SUNOX_BROWSER_PATH to a Chromium-family browser binary.".into(),
    ))
}

fn read_owned_cdp_port(path: &std::path::Path) -> Result<u16, CliError> {
    let contents = std::fs::read_to_string(path)?;
    let port = contents
        .lines()
        .next()
        .ok_or_else(|| CliError::Config("DevToolsActivePort was empty".into()))?
        .parse::<u16>()
        .map_err(|_| CliError::Config("DevToolsActivePort contained an invalid port".into()))?;
    if port == 0 {
        return Err(CliError::Config(
            "DevToolsActivePort contained port zero".into(),
        ));
    }
    Ok(port)
}

pub(crate) fn delete_legacy_profile() -> Result<(), CliError> {
    let Some(project_dirs) = directories::ProjectDirs::from("com", "sunox", "sunox") else {
        return Ok(());
    };
    let path: PathBuf = project_dirs.data_dir().join("captcha-browser-profile");
    terminate_legacy_browsers(&path)?;
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn terminate_legacy_browsers(profile_path: &std::path::Path) -> Result<(), CliError> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let matching = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            is_legacy_captcha_command(process.cmd(), profile_path).then_some(*pid)
        })
        .collect::<Vec<_>>();

    for pid in matching {
        let Some(process) = system.process(pid) else {
            continue;
        };
        process.kill_and_wait().map_err(|error| {
            CliError::Config(format!(
                "failed to terminate legacy captcha browser process {pid}: {error:?}"
            ))
        })?;
    }
    Ok(())
}

fn is_legacy_captcha_command(
    command: &[std::ffi::OsString],
    profile_path: &std::path::Path,
) -> bool {
    let expected_profile =
        std::ffi::OsString::from(format!("--user-data-dir={}", profile_path.display()));
    command
        .iter()
        .any(|argument| argument == "--remote-debugging-port=9233")
        && command.iter().any(|argument| argument == &expected_profile)
}

fn drain_stderr(child: &mut Child) {
    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr).lines();
        tokio::spawn(async move {
            while let Ok(Some(_)) = reader.next_line().await {
                // discard
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{is_legacy_captcha_command, read_owned_cdp_port};

    #[test]
    fn owned_cdp_port_comes_from_private_profile_file() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(file, "43123\n/devtools/browser/owned").expect("write port");

        assert_eq!(read_owned_cdp_port(file.path()).expect("port"), 43123);
    }

    #[test]
    fn owned_cdp_port_rejects_invalid_values() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(file, "not-a-port").expect("write invalid port");

        assert!(read_owned_cdp_port(file.path()).is_err());
    }

    #[test]
    fn legacy_cleanup_matches_only_the_exact_sunox_profile_and_port() {
        let profile = std::path::Path::new("/tmp/sunox/captcha-browser-profile");
        let owned = vec![
            "chrome".into(),
            "--remote-debugging-port=9233".into(),
            "--user-data-dir=/tmp/sunox/captcha-browser-profile".into(),
        ];
        let wrong_port = vec![
            "chrome".into(),
            "--remote-debugging-port=9234".into(),
            "--user-data-dir=/tmp/sunox/captcha-browser-profile".into(),
        ];
        let prefix_collision = vec![
            "chrome".into(),
            "--remote-debugging-port=9233".into(),
            "--user-data-dir=/tmp/sunox/captcha-browser-profile-other".into(),
        ];

        assert!(is_legacy_captcha_command(&owned, profile));
        assert!(!is_legacy_captcha_command(&wrong_port, profile));
        assert!(!is_legacy_captcha_command(&prefix_collision, profile));
    }
}
