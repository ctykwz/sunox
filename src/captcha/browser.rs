use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::time::sleep;

use super::{CDP_PORT, cdp};
use crate::browser::locate_chromium_browser;
use crate::core::CliError;

static BROWSER: OnceLock<Mutex<Option<Child>>> = OnceLock::new();

fn browser_slot() -> &'static Mutex<Option<Child>> {
    BROWSER.get_or_init(|| Mutex::new(None))
}

/// Either reuse a Chromium-family browser already listening on `CDP_PORT` or
/// spawn a new hidden one with the captcha browser profile. Idempotent.
pub(super) async fn ensure_running() -> Result<(), CliError> {
    if cdp::cdp_version().await.is_ok() {
        return Ok(());
    }

    let browser_path = locate_chromium_browser()?;
    let profile_dir = directories::ProjectDirs::from("com", "sunox", "sunox")
        .map(|d| d.data_dir().join("captcha-browser-profile"))
        .ok_or_else(|| CliError::Config("could not resolve data dir for browser profile".into()))?;
    std::fs::create_dir_all(&profile_dir)?;

    eprintln!("Launching browser for captcha solver (one-time per session)...");

    // Do not use --headless. hCaptcha's bot-detection trips on headless mode.
    let mut child = Command::new(&browser_path)
        .arg(format!("--remote-debugging-port={CDP_PORT}"))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
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
        .spawn()
        .map_err(|e| {
            CliError::Config(format!("failed to spawn browser at {browser_path:?}: {e}"))
        })?;
    drain_stderr(&mut child);

    {
        let mut slot = browser_slot().lock().await;
        *slot = Some(child);
    }

    for _ in 0..20 {
        sleep(Duration::from_millis(500)).await;
        if cdp::cdp_version().await.is_ok() {
            return Ok(());
        }
    }

    Err(CliError::Config(
        "Browser was spawned but never opened the CDP port. Check that Chrome or Edge can start normally, or set SUNO_BROWSER_PATH to a Chromium-family browser binary.".into(),
    ))
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
