use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;

use super::AuthState;
use super::clerk_token_exchange;
use super::cookie::{is_suno_auth_cookie_domain, is_suno_cookie_domain, sanitize_device_id};
use super::environment::{
    accept_language_from_browser_languages, accept_language_from_system_locale,
    non_empty_header_value,
};
use super::types::{BrowserAuth, BrowserClientHints, BrowserEnvironment};
use crate::api::SunoClient;
use crate::browser::{
    locate_chromium_browser, locate_chromium_browser_for_source, locate_firefox_browser_for_source,
};
use crate::core::CliError;
use crate::net::http;

const CDP_HOST: &str = "127.0.0.1";
const LOGIN_URL: &str = "https://suno.com/create";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const EXISTING_SESSION_PROBE_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const BROWSER_PROCESS_POLL_INTERVAL: Duration = Duration::from_secs(1);
const BROWSER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const BROWSER_SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(100);
const SESSION_VALIDATION_INTERVAL: Duration = Duration::from_secs(10);
const INTERACTIVE_BROWSER_SOURCE: &str = "interactive-browser";
const BROWSER_STDERR_LINES: usize = 20;

#[derive(Debug, Clone, Deserialize)]
struct CdpCookie {
    name: String,
    value: String,
    domain: String,
}

#[derive(Debug, Deserialize)]
struct CdpTarget {
    #[serde(rename = "type")]
    target_type: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserEnvironmentProbe {
    #[serde(rename = "userAgent")]
    user_agent: Option<String>,
    #[serde(default)]
    languages: Vec<String>,
    #[serde(rename = "userAgentData")]
    user_agent_data: Option<UserAgentDataProbe>,
}

#[derive(Debug, Deserialize)]
struct UserAgentDataProbe {
    #[serde(default)]
    brands: Vec<UserAgentBrandProbe>,
    mobile: bool,
    platform: String,
}

#[derive(Debug, Deserialize)]
struct UserAgentBrandProbe {
    brand: String,
    version: String,
}

#[derive(Serialize)]
struct CdpRequest<'a> {
    id: u64,
    method: &'a str,
    params: serde_json::Value,
}

type CdpStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct CdpSession {
    ws: CdpStream,
    next_id: u64,
}

struct InteractiveBrowserSession {
    child: Child,
    processes: OwnedBrowserProcesses,
    port: u16,
}

impl InteractiveBrowserSession {
    async fn shutdown(mut self) {
        request_browser_close(self.port).await;
        stop_browser_processes(&mut self.child, &mut self.processes).await;
    }
}

async fn request_browser_close(port: u16) {
    let _ = timeout(Duration::from_secs(2), try_request_browser_close(port)).await;
}

async fn try_request_browser_close(port: u16) {
    let Ok(version) = cdp_version(port).await else {
        return;
    };
    let Some(ws_url) = version
        .get("webSocketDebuggerUrl")
        .and_then(serde_json::Value::as_str)
    else {
        return;
    };
    let Ok(ws_url) = validate_and_pin_ws_url(ws_url, port) else {
        return;
    };
    let Ok(mut session) = CdpSession::connect(&ws_url).await else {
        return;
    };
    let _ = session.call("Browser.close", serde_json::json!({})).await;
}

async fn stop_browser_processes(child: &mut Child, processes: &mut OwnedBrowserProcesses) {
    let deadline = tokio::time::Instant::now() + BROWSER_SHUTDOWN_TIMEOUT;
    while !processes.active_pids().is_empty() && tokio::time::Instant::now() < deadline {
        sleep(BROWSER_SHUTDOWN_POLL_INTERVAL).await;
    }
    if !processes.active_pids().is_empty() {
        let _ = child.start_kill();
        processes.terminate();
    }
    let _ = timeout(Duration::from_secs(1), child.wait()).await;
    processes.disarm();
}

impl Drop for InteractiveBrowserSession {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        self.processes.terminate();
        self.processes.disarm();
    }
}

struct OwnedBrowserProcesses {
    known: HashSet<sysinfo::Pid>,
    profile_dir: PathBuf,
    armed: bool,
}

impl OwnedBrowserProcesses {
    fn new(root_pid: Option<u32>, profile_dir: &Path) -> Self {
        Self {
            known: root_pid.map(sysinfo::Pid::from_u32).into_iter().collect(),
            profile_dir: profile_dir.to_path_buf(),
            armed: true,
        }
    }

    fn active_pids(&mut self) -> Vec<sysinfo::Pid> {
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        loop {
            let discovered = system
                .processes()
                .iter()
                .filter_map(|(pid, process)| {
                    (!self.known.contains(pid)
                        && (process
                            .parent()
                            .is_some_and(|parent| self.known.contains(&parent))
                            || process_uses_profile(process.cmd(), &self.profile_dir)))
                    .then_some(*pid)
                })
                .collect::<Vec<_>>();
            if discovered.is_empty() {
                break;
            }
            self.known.extend(discovered);
        }
        self.known
            .iter()
            .copied()
            .filter(|pid| system.process(*pid).is_some())
            .collect()
    }

    fn terminate(&mut self) {
        if !self.armed {
            return;
        }
        let active = self.active_pids();
        if active.is_empty() {
            return;
        }
        let mut system = System::new();
        system.refresh_processes(ProcessesToUpdate::All, true);
        for pid in active {
            if let Some(process) = system.process(pid) {
                let _ = process.kill();
            }
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(windows)]
fn windows_has_visible_window(pids: &HashSet<u32>) -> bool {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
    };
    use windows_sys::core::BOOL;

    struct Search<'a> {
        pids: &'a HashSet<u32>,
        found: bool,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: EnumWindows invokes this callback synchronously while `search`
        // remains alive, and lparam points to that stack value for the full call.
        let search = unsafe { &mut *(lparam as *mut Search<'_>) };
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if search.pids.contains(&pid) {
            search.found = true;
            return 0;
        }
        1
    }

    let mut search = Search { pids, found: false };
    // SAFETY: `visit` satisfies WNDENUMPROC and the callback data remains valid
    // until this synchronous enumeration returns.
    unsafe { EnumWindows(Some(visit), (&mut search as *mut Search<'_>) as LPARAM) };
    search.found
}

impl Drop for OwnedBrowserProcesses {
    fn drop(&mut self) {
        self.terminate();
    }
}

impl CdpSession {
    async fn connect(ws_url: &str) -> Result<Self, CliError> {
        let ws_url = ws_url.to_string();
        let (ws, _) = tokio::spawn(async move { tokio_tungstenite::connect_async(ws_url).await })
            .await
            .map_err(|e| CliError::Config(format!("CDP ws task: {e}")))?
            .map_err(|e| CliError::Config(format!("CDP ws connect: {e}")))?;

        Ok(Self { ws, next_id: 0 })
    }

    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CliError> {
        self.next_id += 1;
        let id = self.next_id;
        let payload = serde_json::to_string(&CdpRequest { id, method, params }).unwrap();

        self.ws
            .send(Message::Text(payload))
            .await
            .map_err(|e| CliError::Config(format!("CDP ws send {method}: {e}")))?;

        loop {
            let msg = timeout(Duration::from_secs(60), self.ws.next())
                .await
                .map_err(|_| CliError::Config(format!("CDP {method} timeout")))?
                .ok_or_else(|| CliError::Config(format!("CDP {method} ws closed")))?
                .map_err(|e| CliError::Config(format!("CDP {method} ws err: {e}")))?;

            let text = match msg {
                Message::Text(text) => text.to_string(),
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
                    continue;
                }
                Message::Close(_) => {
                    return Err(CliError::Config(format!("CDP {method} ws closed mid-call")));
                }
            };

            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| CliError::Config(format!("CDP {method} json: {e}")))?;
            if value.get("id").and_then(|id| id.as_u64()) == Some(id) {
                if let Some(err) = value.get("error") {
                    return Err(CliError::Config(format!("CDP {method} error: {err}")));
                }
                return Ok(value
                    .get("result")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null));
            }
        }
    }
}

pub async fn extract_interactive_browser_auth() -> Result<(BrowserAuth, String, String), CliError> {
    let (browser_path, profile_dir) = prepare_interactive_browser_profile()?;
    if dedicated_profile_has_cookie_database(&profile_dir) {
        eprintln!("Checking the existing dedicated browser session...");
        let session = spawn_login_browser(&browser_path, &profile_dir).await?;
        let port = session.port;
        let probe = timeout(EXISTING_SESSION_PROBE_TIMEOUT, async {
            eprintln!("Dedicated browser CDP is ready; reading the active page...");
            let version_environment = browser_environment_from_cdp_version(port).await;
            let ws_url = find_or_create_login_tab(port).await?;
            eprintln!("Dedicated browser page found; validating stored cookies...");
            wait_for_suno_auth(ws_url, version_environment, false).await
        })
        .await;
        session.shutdown().await;
        match probe {
            Ok(Ok(auth)) => {
                eprintln!("Existing dedicated browser session verified.");
                return Ok(auth);
            }
            Ok(Err(error)) => {
                eprintln!("Existing dedicated browser session was rejected: {error}");
            }
            Err(_) => {
                eprintln!("Existing dedicated browser session validation timed out.");
            }
        }
        eprintln!("No usable session was found in the dedicated profile; opening manual login...");
    }
    run_manual_login_browser(&browser_path, &profile_dir).await?;
    cleanup_stale_profile_lock(&profile_dir)?;
    let session = spawn_login_browser(&browser_path, &profile_dir).await?;
    let port = session.port;

    let result = async {
        let version_environment = browser_environment_from_cdp_version(port).await;
        let ws_url = find_or_create_login_tab(port).await?;
        eprintln!("Complete Suno login in the opened browser window...");
        wait_for_suno_auth(ws_url, version_environment, true).await
    }
    .await;

    if result.is_ok() {
        eprintln!("Suno login captured; closing the dedicated browser window.");
    }
    session.shutdown().await;
    result
}

/// Read the browser's actual runtime User-Agent without opening a visible
/// window or navigating to Suno. This is used to repair legacy/cookie-derived
/// auth states whose profile data cannot contain a runtime-generated UA.
pub(crate) async fn probe_browser_runtime_environment(
    browser_source: &str,
) -> Result<BrowserEnvironment, CliError> {
    let browser_path = locate_chromium_browser_for_source(browser_source)?;
    let profile = tempfile::Builder::new()
        .prefix("sunox-browser-probe-")
        .tempdir()
        .map_err(CliError::Io)?;
    let profile_dir = profile.path();
    let active_port_path = profile_dir.join("DevToolsActivePort");

    let mut command = Command::new(&browser_path);
    command
        .arg("--headless=new")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-extensions")
        .arg("--disable-background-mode")
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        CliError::Config(format!(
            "failed to start browser metadata probe at {browser_path:?}: {error}"
        ))
    })?;
    let mut processes = OwnedBrowserProcesses::new(child.id(), profile_dir);

    let result = timeout(Duration::from_secs(12), async {
        let mut last_error = None;
        for _ in 0..40 {
            let _ = processes.active_pids();
            if let Some(status) = child.try_wait()? {
                return Err(CliError::Config(format!(
                    "browser metadata probe exited before CDP became ready: {status}"
                )));
            }
            match read_owned_cdp_port(&active_port_path) {
                Ok(port) => {
                    let version_environment = browser_environment_from_cdp_version(port).await;
                    let page_environment = match cdp_list(port).await {
                        Ok(targets) => {
                            let ws_url = targets.into_iter().find_map(|target| {
                                (target.target_type == "page")
                                    .then_some(target.web_socket_debugger_url)
                                    .flatten()
                                    .and_then(|url| validate_and_pin_ws_url(&url, port).ok())
                            });
                            match ws_url {
                                Some(ws_url) => match CdpSession::connect(&ws_url).await {
                                    Ok(mut session) => {
                                        let mut environment =
                                            browser_environment_from_page(&mut session).await;
                                        if environment
                                            .as_ref()
                                            .and_then(|value| value.client_hints.as_ref())
                                            .is_none()
                                        {
                                            let _ = session
                                                .call(
                                                    "Page.navigate",
                                                    serde_json::json!({
                                                        "url": format!("http://{CDP_HOST}:{port}/json/version")
                                                    }),
                                                )
                                                .await;
                                            sleep(Duration::from_millis(100)).await;
                                            environment = merge_browser_environments(
                                                browser_environment_from_page(&mut session).await,
                                                environment,
                                            );
                                        }
                                        environment
                                    }
                                    Err(error) => {
                                        last_error = Some(error.to_string());
                                        None
                                    }
                                },
                                None => {
                                    last_error = Some("CDP returned no blank page target".into());
                                    None
                                }
                            }
                        }
                        Err(error) => {
                            last_error = Some(error.to_string());
                            None
                        }
                    };
                    if let Some(mut environment) =
                        merge_browser_environments(page_environment, version_environment)
                    {
                        environment.browser_source = Some(browser_source.to_string());
                        environment.user_agent = environment
                            .user_agent
                            .map(|value| normalize_runtime_user_agent(&value));
                        if environment.user_agent.is_some()
                            || environment.accept_language.is_some()
                            || environment.client_hints.is_some()
                        {
                            return Ok(environment);
                        }
                        last_error = Some(
                            "CDP returned incomplete User-Agent, language, or client-hint metadata"
                                .into(),
                        );
                    }
                }
                Err(error) => last_error = Some(error.to_string()),
            }
            sleep(Duration::from_millis(250)).await;
        }
        Err(CliError::Config(format!(
            "browser metadata probe never exposed a usable CDP endpoint: {}",
            last_error.unwrap_or_else(|| "DevToolsActivePort was not created".into())
        )))
    })
    .await
    .unwrap_or_else(|_| {
        Err(CliError::Config(
            "browser metadata probe exceeded its 12-second deadline".into(),
        ))
    });

    if let Ok(port) = read_owned_cdp_port(&active_port_path) {
        request_browser_close(port).await;
    }
    stop_browser_processes(&mut child, &mut processes).await;
    result
}

/// Read Firefox's own negotiated runtime User-Agent through its loopback-only
/// WebDriver BiDi endpoint. `session.new` returns the real UA in capabilities,
/// so no browser version string is synthesized by the CLI.
pub(crate) async fn probe_firefox_runtime_environment(
    browser_source: &str,
) -> Result<BrowserEnvironment, CliError> {
    let browser_path = locate_firefox_browser_for_source(browser_source)?;
    let profile = tempfile::Builder::new()
        .prefix("sunox-firefox-probe-")
        .tempdir()
        .map_err(CliError::Io)?;
    let profile_dir = profile.path();

    let mut command = Command::new(&browser_path);
    command
        .arg("--headless")
        .arg("--no-remote")
        .arg("--new-instance")
        .arg("--profile")
        .arg(profile_dir)
        .arg("--remote-debugging-port")
        .arg("0")
        .arg("about:blank")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        CliError::Config(format!(
            "failed to start Firefox metadata probe at {browser_path:?}: {error}"
        ))
    })?;
    let mut processes = OwnedBrowserProcesses::new(child.id(), profile_dir);
    let stderr_tail = drain_stderr(&mut child);

    let result = timeout(Duration::from_secs(12), async {
        let mut last_error = None;
        for _ in 0..40 {
            let _ = processes.active_pids();
            if let Some(status) = child.try_wait()? {
                return Err(browser_startup_error(
                    format!(
                        "Firefox metadata probe exited before WebDriver BiDi became ready: {status}"
                    ),
                    &stderr_tail,
                ));
            }
            let port = stderr_tail
                .lock()
                .ok()
                .and_then(|lines| firefox_bidi_port(&lines));
            let Some(port) = port else {
                last_error = Some("Firefox has not announced its BiDi port yet".into());
                sleep(Duration::from_millis(250)).await;
                continue;
            };
            let ws_url = format!("ws://{CDP_HOST}:{port}/session");
            match CdpSession::connect(&ws_url).await {
                Ok(mut session) => match session
                    .call("session.new", serde_json::json!({ "capabilities": {} }))
                    .await
                {
                    Ok(value) => {
                        let mut environment =
                            firefox_environment_from_session_new(&value, browser_source)?;
                        if let Some(accept_language) =
                            firefox_accept_language_from_bidi(&mut session).await
                        {
                            environment.accept_language = Some(accept_language);
                        }
                        let _ = timeout(
                            Duration::from_secs(2),
                            session.call("browser.close", serde_json::json!({})),
                        )
                        .await;
                        return Ok(environment);
                    }
                    Err(error) => last_error = Some(error.to_string()),
                },
                Err(error) => last_error = Some(error.to_string()),
            }
            sleep(Duration::from_millis(250)).await;
        }
        Err(CliError::Config(format!(
            "Firefox metadata probe never exposed a usable WebDriver BiDi endpoint: {}",
            last_error.unwrap_or_else(|| "loopback endpoint was not ready".into())
        )))
    })
    .await
    .unwrap_or_else(|_| {
        Err(CliError::Config(
            "Firefox metadata probe exceeded its 12-second deadline".into(),
        ))
    });

    stop_browser_processes(&mut child, &mut processes).await;
    result
}

async fn firefox_accept_language_from_bidi(session: &mut CdpSession) -> Option<String> {
    let tree = session
        .call("browsingContext.getTree", serde_json::json!({}))
        .await
        .ok()?;
    let context = tree
        .get("contexts")?
        .as_array()?
        .first()?
        .get("context")?
        .as_str()?;
    let evaluated = session
        .call(
            "script.evaluate",
            serde_json::json!({
                "expression": "JSON.stringify(Array.from(navigator.languages || [navigator.language]).filter(Boolean))",
                "target": { "context": context },
                "awaitPromise": false,
                "resultOwnership": "none",
                "userActivation": false
            }),
        )
        .await
        .ok()?;
    let payload = evaluated.get("result")?.get("value")?.as_str()?;
    let languages = serde_json::from_str::<Vec<String>>(payload).ok()?;
    accept_language_from_browser_languages(&languages)
}

fn firefox_bidi_port(lines: &VecDeque<String>) -> Option<u16> {
    const PREFIX: &str = "WebDriver BiDi listening on ws://127.0.0.1:";
    lines.iter().rev().find_map(|line| {
        let port = line.split_once(PREFIX)?.1;
        let port = port.split('/').next().unwrap_or(port).trim();
        port.parse::<u16>().ok().filter(|port| *port != 0)
    })
}

fn firefox_environment_from_session_new(
    result: &serde_json::Value,
    browser_source: &str,
) -> Result<BrowserEnvironment, CliError> {
    let user_agent = result
        .get("capabilities")
        .and_then(|capabilities| capabilities.get("userAgent"))
        .and_then(serde_json::Value::as_str)
        .and_then(|value| non_empty_header_value(Some(value)))
        .ok_or_else(|| {
            CliError::Config("Firefox WebDriver BiDi returned no runtime User-Agent".into())
        })?;
    Ok(BrowserEnvironment {
        browser_source: Some(browser_source.to_string()),
        user_agent: Some(user_agent),
        accept_language: accept_language_from_system_locale(),
        client_hints: None,
    })
}

pub(crate) fn recorded_interactive_browser_source() -> Option<String> {
    let identity_file = directories::ProjectDirs::from("com", "sunox", "sunox")?
        .data_local_dir()
        .join("interactive-login-browser-profile")
        .join("sunox-browser-path.txt");
    let browser_path = std::fs::read_to_string(identity_file).ok()?;
    crate::browser::browser_source_for_path(Path::new(browser_path.trim())).map(str::to_string)
}

fn normalize_runtime_user_agent(user_agent: &str) -> String {
    user_agent.replace("HeadlessChrome/", "Chrome/")
}

fn dedicated_profile_has_cookie_database(profile_dir: &Path) -> bool {
    [
        profile_dir.join("Default").join("Network").join("Cookies"),
        profile_dir.join("Default").join("Cookies"),
    ]
    .into_iter()
    .any(|path| path.is_file())
}

pub fn delete_interactive_browser_profile() -> Result<(), CliError> {
    let Some(project_dirs) = directories::ProjectDirs::from("com", "sunox", "sunox") else {
        // Logout must remain usable in headless/minimal environments where the
        // platform cannot resolve a data directory. In that case no profile
        // path can have been created by this process, so there is nothing to
        // remove.
        return Ok(());
    };
    let local_profile = project_dirs
        .data_local_dir()
        .join("interactive-login-browser-profile");
    let roaming_profile = project_dirs
        .data_dir()
        .join("interactive-login-browser-profile");
    delete_interactive_browser_profile_at(&local_profile)?;
    if roaming_profile != local_profile {
        delete_interactive_browser_profile_at(&roaming_profile)?;
    }
    Ok(())
}

fn prepare_interactive_browser_profile() -> Result<(PathBuf, PathBuf), CliError> {
    let browser_path = locate_chromium_browser()?;
    let profile_dir = interactive_browser_profile_dir()?;
    std::fs::create_dir_all(&profile_dir)?;
    terminate_profile_browsers(&profile_dir)?;
    cleanup_stale_profile_lock(&profile_dir)?;
    validate_profile_browser_identity(&profile_dir, &browser_path)?;
    Ok((browser_path, profile_dir))
}

fn cleanup_stale_profile_lock(profile_dir: &Path) -> Result<(), CliError> {
    #[cfg(windows)]
    {
        let lock = profile_dir.join("lockfile");
        if lock.exists() {
            std::fs::remove_file(&lock).map_err(|error| {
                CliError::Config(format!(
                    "the dedicated browser profile is still locked ({}): {error}",
                    lock.display()
                ))
            })?;
        }
    }
    #[cfg(not(windows))]
    let _ = profile_dir;
    Ok(())
}

async fn run_manual_login_browser(browser_path: &Path, profile_dir: &Path) -> Result<(), CliError> {
    eprintln!(
        "Opening Chrome without remote debugging so Google login is allowed. Complete Suno login, then close that browser window to continue..."
    );
    let mut command = Command::new(browser_path);
    command
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-features=TranslateUI")
        .arg("--disable-extensions")
        .arg("--disable-background-mode")
        .arg("--window-size=1280,900")
        .arg(LOGIN_URL)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        CliError::Config(format!(
            "failed to spawn manual login browser at {browser_path:?}: {error}"
        ))
    })?;
    let mut processes = OwnedBrowserProcesses::new(child.id(), profile_dir);
    let stderr_tail = drain_stderr(&mut child);
    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    let startup_deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut browser_seen = false;
    #[cfg(windows)]
    let mut window_seen = false;

    loop {
        let matching = processes.active_pids();
        #[cfg(windows)]
        let visible_window = {
            let pids = matching
                .iter()
                .map(|pid| pid.as_u32())
                .collect::<HashSet<_>>();
            windows_has_visible_window(&pids)
        };
        let child_status = child.try_wait()?;
        if child_status.is_none() || !matching.is_empty() {
            browser_seen = true;
            record_profile_browser_identity(profile_dir, browser_path)?;
        }
        #[cfg(windows)]
        {
            window_seen |= visible_window;
            if window_seen && !visible_window {
                stop_browser_processes(&mut child, &mut processes).await;
                eprintln!("Manual login browser closed; verifying the Suno session...");
                return Ok(());
            }
        }
        #[cfg(not(windows))]
        if manual_browser_has_closed(browser_seen, child_status.is_some(), matching.is_empty()) {
            let _ = child.wait().await;
            eprintln!("Manual login browser closed; verifying the Suno session...");
            return Ok(());
        }
        if !browser_seen
            && let Some(status) = child_status
            && !status.success()
        {
            return Err(browser_startup_error(
                format!("manual login browser exited before opening: {status}"),
                &stderr_tail,
            ));
        }
        if !browser_seen && tokio::time::Instant::now() >= startup_deadline {
            return Err(browser_startup_error(
                "manual login browser did not remain open or expose an owned process".into(),
                &stderr_tail,
            ));
        }

        if tokio::time::Instant::now() >= deadline {
            stop_browser_processes(&mut child, &mut processes).await;
            return Err(CliError::Config(
                "Timed out waiting for the manual Suno login browser to be closed.".into(),
            ));
        }
        sleep(BROWSER_PROCESS_POLL_INTERVAL).await;
    }
}

#[cfg(any(not(windows), test))]
fn manual_browser_has_closed(
    browser_seen: bool,
    child_exited: bool,
    no_owned_processes: bool,
) -> bool {
    browser_seen && child_exited && no_owned_processes
}

async fn spawn_login_browser(
    browser_path: &Path,
    profile_dir: &Path,
) -> Result<InteractiveBrowserSession, CliError> {
    let active_port_path = profile_dir.join("DevToolsActivePort");
    if active_port_path.exists() {
        std::fs::remove_file(&active_port_path)?;
    }

    eprintln!(
        "Opening a dedicated browser profile for Suno login. This avoids reading your default browser cookies."
    );

    let mut command = Command::new(browser_path);
    command
        .arg("--remote-debugging-address=127.0.0.1")
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-features=TranslateUI")
        .arg("--disable-extensions")
        .arg("--disable-background-mode")
        .arg("--window-size=1280,900")
        .arg(LOGIN_URL)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|e| {
        CliError::Config(format!("failed to spawn browser at {browser_path:?}: {e}"))
    })?;
    let mut processes = OwnedBrowserProcesses::new(child.id(), profile_dir);
    let stderr_tail = drain_stderr(&mut child);

    let mut last_error = None;
    for _ in 0..40 {
        let _ = processes.active_pids();
        if let Some(status) = child.try_wait()?
            && !status.success()
        {
            return Err(browser_startup_error(
                format!("browser exited before CDP became ready: {status}"),
                &stderr_tail,
            ));
        }
        match read_owned_cdp_port(&active_port_path) {
            Ok(port) => match cdp_version(port).await {
                Ok(_) => {
                    record_profile_browser_identity(profile_dir, browser_path)?;
                    return Ok(InteractiveBrowserSession {
                        child,
                        processes,
                        port,
                    });
                }
                Err(error) => last_error = Some(error.to_string()),
            },
            Err(error) => last_error = Some(error.to_string()),
        }
        sleep(Duration::from_millis(250)).await;
    }

    stop_browser_processes(&mut child, &mut processes).await;
    let detail = last_error.unwrap_or_else(|| "DevToolsActivePort was not created".into());
    Err(browser_startup_error(
        format!("browser never exposed its owned CDP endpoint: {detail}"),
        &stderr_tail,
    ))
}

fn interactive_browser_profile_dir() -> Result<PathBuf, CliError> {
    let dirs = directories::ProjectDirs::from("com", "sunox", "sunox")
        .ok_or_else(|| CliError::Config("could not resolve data dir for browser profile".into()))?;
    let local = dirs
        .data_local_dir()
        .join("interactive-login-browser-profile");
    let roaming = dirs.data_dir().join("interactive-login-browser-profile");

    migrate_legacy_profile(&roaming, &local)?;
    Ok(local)
}

fn migrate_legacy_profile(roaming: &Path, local: &Path) -> Result<(), CliError> {
    if local == roaming || local.exists() || !roaming.exists() {
        return Ok(());
    }
    if let Some(parent) = local.parent() {
        std::fs::create_dir_all(parent)?;
    }
    terminate_profile_browsers(roaming)?;
    match std::fs::rename(roaming, local) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device_error(&error) => {
            copy_profile_across_volumes(roaming, local)?;
            if let Err(cleanup_error) = std::fs::remove_dir_all(roaming) {
                eprintln!(
                    "Warning: migrated the dedicated login profile to Local AppData but could not remove the old Roaming copy: {cleanup_error}"
                );
            }
            Ok(())
        }
        Err(error) => Err(CliError::Config(format!(
            "could not migrate the dedicated login profile from Roaming to Local AppData: {error}"
        ))),
    }
}

fn is_cross_device_error(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::CrossesDevices
        || (cfg!(windows) && error.raw_os_error() == Some(17))
        || (cfg!(unix) && error.raw_os_error() == Some(18))
}

fn copy_profile_across_volumes(source: &Path, destination: &Path) -> Result<(), CliError> {
    let parent = destination.parent().ok_or_else(|| {
        CliError::Config("Local AppData browser profile has no parent directory".into())
    })?;
    let staging = parent.join(format!(
        ".interactive-login-browser-profile.{}.migration",
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let source_stats = copy_profile_tree(source, &staging)?;
        let copied_stats = profile_tree_stats(&staging)?;
        if source_stats != copied_stats {
            return Err(CliError::Config(format!(
                "browser profile migration verification failed: source {source_stats:?}, copied {copied_stats:?}"
            )));
        }
        std::fs::rename(&staging, destination)?;
        Ok(())
    })();
    if result.is_err() && staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProfileTreeStats {
    files: u64,
    bytes: u64,
}

fn copy_profile_tree(source: &Path, destination: &Path) -> Result<ProfileTreeStats, CliError> {
    std::fs::create_dir(destination)?;
    let mut stats = ProfileTreeStats { files: 0, bytes: 0 };
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&source_path)?;
        if profile_entry_is_link(&metadata) {
            return Err(CliError::Config(format!(
                "browser profile migration refused symbolic link: {}",
                source_path.display()
            )));
        }
        if metadata.is_dir() {
            let child = copy_profile_tree(&source_path, &destination_path)?;
            stats.files += child.files;
            stats.bytes += child.bytes;
        } else if metadata.is_file() {
            let copied = std::fs::copy(&source_path, &destination_path)?;
            stats.files += 1;
            stats.bytes += copied;
        }
    }
    Ok(stats)
}

fn profile_tree_stats(root: &Path) -> Result<ProfileTreeStats, CliError> {
    let mut stats = ProfileTreeStats { files: 0, bytes: 0 };
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        if profile_entry_is_link(&metadata) {
            return Err(CliError::Config(format!(
                "copied browser profile unexpectedly contains a symbolic link: {}",
                path.display()
            )));
        }
        if metadata.is_dir() {
            let child = profile_tree_stats(&path)?;
            stats.files += child.files;
            stats.bytes += child.bytes;
        } else if metadata.is_file() {
            stats.files += 1;
            stats.bytes += metadata.len();
        }
    }
    Ok(stats)
}

fn profile_entry_is_link(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

fn validate_profile_browser_identity(
    profile_dir: &Path,
    browser_path: &Path,
) -> Result<(), CliError> {
    let identity_file = profile_dir.join("sunox-browser-path.txt");
    if !identity_file.exists() {
        return Ok(());
    }
    let browser_identity = std::fs::canonicalize(browser_path)
        .unwrap_or_else(|_| browser_path.to_path_buf())
        .display()
        .to_string();
    let existing = std::fs::read_to_string(&identity_file)?;
    if existing.trim() != browser_identity {
        return Err(CliError::Config(format!(
            "the dedicated login profile belongs to a different browser ({existing}). Run `sunox logout` before switching to {}",
            browser_path.display()
        )));
    }
    Ok(())
}

fn record_profile_browser_identity(
    profile_dir: &Path,
    browser_path: &Path,
) -> Result<(), CliError> {
    validate_profile_browser_identity(profile_dir, browser_path)?;
    let identity_file = profile_dir.join("sunox-browser-path.txt");
    if identity_file.exists() {
        return Ok(());
    }
    let browser_identity = std::fs::canonicalize(browser_path)
        .unwrap_or_else(|_| browser_path.to_path_buf())
        .display()
        .to_string();
    std::fs::write(identity_file, browser_identity)?;
    Ok(())
}

fn delete_interactive_browser_profile_at(profile_dir: &Path) -> Result<(), CliError> {
    terminate_profile_browsers(profile_dir)?;
    if profile_dir.exists() {
        std::fs::remove_dir_all(profile_dir)?;
    }
    Ok(())
}

async fn cdp_version(port: u16) -> Result<serde_json::Value, CliError> {
    let url = format!("http://{CDP_HOST}:{port}/json/version");
    http::loopback_client()?
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/version: {e}")))?
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP json parse: {e}")))
}

async fn find_or_create_login_tab(port: u16) -> Result<String, CliError> {
    let targets = cdp_list(port).await?;
    if let Some(ws_url) = targets.into_iter().find_map(|target| {
        if target.target_type == "page" && !target.url.starts_with("chrome://") {
            target
                .web_socket_debugger_url
                .and_then(|url| validate_and_pin_ws_url(&url, port).ok())
        } else {
            None
        }
    }) {
        return Ok(ws_url);
    }

    let url = format!("http://{CDP_HOST}:{port}/json/new?{}", urlencode(LOGIN_URL));
    let target: CdpTarget = http::loopback_client()?
        .put(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/new: {e}")))?
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/new parse: {e}")))?;
    let ws_url = target
        .web_socket_debugger_url
        .ok_or_else(|| CliError::Config("CDP /json/new did not return a websocket URL".into()))?;
    validate_and_pin_ws_url(&ws_url, port)
}

async fn cdp_list(port: u16) -> Result<Vec<CdpTarget>, CliError> {
    let url = format!("http://{CDP_HOST}:{port}/json/list");
    http::loopback_client()?
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/list: {e}")))?
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP json parse: {e}")))
}

async fn wait_for_suno_auth(
    ws_url: String,
    fallback_environment: Option<BrowserEnvironment>,
    retry_rejected_auth: bool,
) -> Result<(BrowserAuth, String, String), CliError> {
    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    eprintln!("Preparing the Suno validation client...");
    let http = http::browser_client()?;
    eprintln!("Connecting to the dedicated browser page...");
    let mut session = CdpSession::connect(&ws_url).await?;
    // The browser is launched directly on LOGIN_URL. Enabling Page/Network and
    // navigating again floods the page CDP socket with unrelated events before
    // the cookie response, which can exhaust the small async worker stack on a
    // busy signed-in profile. These commands work without enabling the domains.
    eprintln!("Reading the dedicated browser environment...");
    let browser_environment = merge_browser_environments(
        browser_environment_from_page(&mut session).await,
        fallback_environment,
    )
    .or_else(|| Some(interactive_browser_environment()));
    eprintln!("Requesting Suno cookies from the dedicated browser...");
    let mut last_validation: Option<(String, tokio::time::Instant)> = None;
    let mut last_token_error: Option<String> = None;

    loop {
        if tokio::time::Instant::now() >= deadline {
            let detail = last_token_error
                .map(|error| format!(" Last token validation error: {error}"))
                .unwrap_or_default();
            return Err(CliError::Config(format!(
                "Timed out waiting for a Suno-accepted login token in the dedicated browser window.{detail}"
            )));
        }

        let result = session
            .call(
                "Network.getCookies",
                serde_json::json!({
                    "urls": [
                        "https://suno.com/",
                        "https://auth.suno.com/",
                        "https://studio.suno.com/",
                        "https://app.suno.ai/"
                    ]
                }),
            )
            .await?;
        let cookies: Vec<CdpCookie> =
            serde_json::from_value(result.get("cookies").cloned().unwrap_or_default())
                .map_err(|e| CliError::Config(format!("CDP cookie parse: {e}")))?;
        if last_validation.is_none() {
            let visible = cookies
                .iter()
                .map(|cookie| format!("{}:{}", cookie.domain, cookie.name))
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("CDP returned relevant cookies: {visible}");
        }
        if let Some(auth) = browser_auth_from_cdp_cookies(cookies, browser_environment.clone()) {
            let now = tokio::time::Instant::now();
            let should_validate = last_validation.as_ref().is_none_or(|(cookie, checked_at)| {
                cookie != &auth.clerk_client_cookie
                    || now.duration_since(*checked_at) >= SESSION_VALIDATION_INTERVAL
            });
            if should_validate {
                last_validation = Some((auth.clerk_client_cookie.clone(), now));
                match clerk_token_exchange(
                    &http,
                    &auth.clerk_client_cookie,
                    auth.browser_environment.as_ref(),
                )
                .await
                {
                    Ok((session_id, jwt)) => {
                        eprintln!("Clerk issued a JWT; verifying it with the Suno API...");
                        match validate_suno_login(&auth, &session_id, &jwt).await {
                            Ok(()) => return Ok((auth, session_id, jwt)),
                            Err(error)
                                if transient_login_error(&error)
                                    || retry_rejected_auth
                                        && interactive_login_should_retry(&error) =>
                            {
                                last_token_error = Some(error.to_string());
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    Err(error)
                        if transient_login_error(&error)
                            || retry_rejected_auth && interactive_login_should_retry(&error) =>
                    {
                        last_token_error = Some(error.to_string());
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        sleep(POLL_INTERVAL).await;
    }
}

async fn validate_suno_login(
    auth: &BrowserAuth,
    session_id: &str,
    jwt: &str,
) -> Result<(), CliError> {
    let state = AuthState {
        jwt: Some(jwt.to_string()),
        cookie: Some(auth.cookie_header.clone()),
        session_id: Some(session_id.to_string()),
        device_id: auth.device_id.clone(),
        browser_environment: auth.browser_environment.clone(),
        clerk_client_cookie: Some(auth.clerk_client_cookie.clone()),
    };
    SunoClient::new_for_auth_validation(state)?
        .validate_auth()
        .await
}

fn interactive_login_should_retry(error: &CliError) -> bool {
    match error {
        CliError::Http(_) | CliError::AuthExpired | CliError::RateLimited => true,
        CliError::Api { code, .. } => matches!(
            *code,
            "no_session"
                | "no_jwt"
                | "clerk_exchange_rejected"
                | "clerk_exchange_failed"
                | "clerk_refresh_rejected"
                | "clerk_refresh_failed"
                | "clerk_rate_limited"
        ),
        CliError::SunoApi {
            status, retryable, ..
        } => *status >= 500 || *retryable == Some(true),
        _ => false,
    }
}

fn transient_login_error(error: &CliError) -> bool {
    match error {
        CliError::Http(_) | CliError::RateLimited => true,
        CliError::Api { code, .. } => matches!(
            *code,
            "clerk_exchange_failed" | "clerk_refresh_failed" | "clerk_rate_limited"
        ),
        CliError::SunoApi {
            status, retryable, ..
        } => *status >= 500 || *retryable == Some(true),
        _ => false,
    }
}

async fn browser_environment_from_cdp_version(port: u16) -> Option<BrowserEnvironment> {
    let value = cdp_version(port).await.ok()?;
    browser_environment_from_cdp_version_value(&value)
}

fn browser_environment_from_cdp_version_value(
    value: &serde_json::Value,
) -> Option<BrowserEnvironment> {
    let user_agent = non_empty_header_value(
        value
            .get("User-Agent")
            .or_else(|| value.get("userAgent"))
            .and_then(|user_agent| user_agent.as_str()),
    );
    if user_agent.is_none() {
        None
    } else {
        Some(BrowserEnvironment {
            browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
            user_agent,
            accept_language: None,
            client_hints: None,
        })
    }
}

async fn browser_environment_from_page(session: &mut CdpSession) -> Option<BrowserEnvironment> {
    let result = session
        .call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "JSON.stringify({ userAgent: navigator.userAgent, languages: Array.from(navigator.languages || [navigator.language]).filter(Boolean), userAgentData: navigator.userAgentData ? { brands: Array.from(navigator.userAgentData.brands || []), mobile: Boolean(navigator.userAgentData.mobile), platform: navigator.userAgentData.platform || '' } : null })",
                "returnByValue": true
            }),
        )
        .await
        .ok()?;
    let payload = result
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(|value| value.as_str())?;
    let probe: BrowserEnvironmentProbe = serde_json::from_str(payload).ok()?;
    let user_agent = non_empty_header_value(probe.user_agent.as_deref());
    let accept_language = accept_language_from_browser_languages(&probe.languages);
    let client_hints = probe
        .user_agent_data
        .and_then(browser_client_hints_from_probe);

    if user_agent.is_none() && accept_language.is_none() && client_hints.is_none() {
        None
    } else {
        Some(BrowserEnvironment {
            browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
            user_agent,
            accept_language,
            client_hints,
        })
    }
}

fn browser_client_hints_from_probe(probe: UserAgentDataProbe) -> Option<BrowserClientHints> {
    let brands = probe
        .brands
        .into_iter()
        .filter(|brand| !brand.brand.is_empty() && !brand.version.is_empty())
        .map(|brand| {
            format!(
                "\"{}\";v=\"{}\"",
                escape_client_hint_value(&brand.brand),
                escape_client_hint_value(&brand.version)
            )
        })
        .collect::<Vec<_>>();
    if brands.is_empty() || probe.platform.is_empty() {
        return None;
    }
    Some(BrowserClientHints {
        sec_ch_ua: brands.join(", "),
        sec_ch_ua_mobile: if probe.mobile { "?1" } else { "?0" }.into(),
        sec_ch_ua_platform: format!("\"{}\"", escape_client_hint_value(&probe.platform)),
    })
}

fn escape_client_hint_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn merge_browser_environments(
    primary: Option<BrowserEnvironment>,
    fallback: Option<BrowserEnvironment>,
) -> Option<BrowserEnvironment> {
    match (primary, fallback) {
        (Some(primary), Some(fallback)) => Some(BrowserEnvironment {
            browser_source: primary.browser_source.or(fallback.browser_source),
            user_agent: primary.user_agent.or(fallback.user_agent),
            accept_language: primary.accept_language.or(fallback.accept_language),
            client_hints: primary.client_hints.or(fallback.client_hints),
        }),
        (Some(environment), None) | (None, Some(environment)) => Some(environment),
        (None, None) => None,
    }
}

fn interactive_browser_environment() -> BrowserEnvironment {
    BrowserEnvironment {
        browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
        user_agent: None,
        accept_language: None,
        client_hints: None,
    }
}

fn browser_auth_from_cdp_cookies(
    cookies: Vec<CdpCookie>,
    browser_environment: Option<BrowserEnvironment>,
) -> Option<BrowserAuth> {
    let mut selected: HashMap<String, CdpCookie> = HashMap::new();
    let mut clerk_client_cookie: Option<String> = None;
    let mut auth_domain_clerk: Option<String> = None;
    let mut device_id: Option<String> = None;

    for cookie in cookies {
        if !is_suno_cookie_domain(&cookie.domain)
            || cookie.name.is_empty()
            || cookie.value.is_empty()
        {
            continue;
        }

        if cookie.name == "__client" {
            if is_suno_auth_cookie_domain(&cookie.domain) {
                auth_domain_clerk = Some(cookie.value.clone());
            } else if clerk_client_cookie.is_none() {
                clerk_client_cookie = Some(cookie.value.clone());
            }
        }
        if cookie.name == "ajs_anonymous_id" && device_id.is_none() {
            device_id = sanitize_device_id(&cookie.value);
        }

        match selected.get(&cookie.name) {
            Some(existing)
                if is_suno_auth_cookie_domain(&existing.domain)
                    || !is_suno_auth_cookie_domain(&cookie.domain) => {}
            _ => {
                selected.insert(cookie.name.clone(), cookie);
            }
        }
    }

    let clerk_client_cookie = auth_domain_clerk.or(clerk_client_cookie)?;
    let mut emitted = HashSet::new();
    let mut header_parts = Vec::new();
    if let Some(client_cookie) = selected.get("__client") {
        header_parts.push(format!("__client={}", client_cookie.value));
        emitted.insert("__client".to_string());
    }

    let mut rest: Vec<_> = selected
        .into_values()
        .filter(|cookie| !emitted.contains(&cookie.name))
        .collect();
    rest.sort_by(|a, b| a.name.cmp(&b.name));
    header_parts.extend(
        rest.into_iter()
            .map(|cookie| format!("{}={}", cookie.name, cookie.value)),
    );

    Some(BrowserAuth {
        clerk_client_cookie,
        cookie_header: header_parts.join("; "),
        device_id,
        browser_environment,
    })
}

fn read_owned_cdp_port(path: &Path) -> Result<u16, CliError> {
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

fn validate_and_pin_ws_url(ws_url: &str, expected_port: u16) -> Result<String, CliError> {
    let mut url = reqwest::Url::parse(ws_url)
        .map_err(|_| CliError::Config("CDP returned an invalid websocket URL".into()))?;
    if url.scheme() != "ws"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port() != Some(expected_port)
        || !url.path().starts_with("/devtools/")
    {
        return Err(CliError::Config(
            "CDP websocket endpoint did not match the owned browser".into(),
        ));
    }
    let host_is_loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !host_is_loopback {
        return Err(CliError::Config(
            "CDP websocket endpoint was not loopback".into(),
        ));
    }
    url.set_host(Some(CDP_HOST))
        .map_err(|_| CliError::Config("could not pin CDP websocket to loopback".into()))?;
    Ok(url.into())
}

fn terminate_profile_browsers(profile_path: &Path) -> Result<(), CliError> {
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::All, true);
    let matching = system
        .processes()
        .iter()
        .filter_map(|(pid, process)| {
            process_uses_profile(process.cmd(), profile_path).then_some(*pid)
        })
        .collect::<Vec<_>>();

    for pid in matching {
        let Some(process) = system.process(pid) else {
            continue;
        };
        if !process.kill() {
            return Err(CliError::Config(format!(
                "failed to terminate dedicated login browser process {pid}"
            )));
        }
    }
    Ok(())
}

fn process_uses_profile(arguments: &[std::ffi::OsString], profile_path: &Path) -> bool {
    let expected = normalized_path_text(profile_path);
    arguments.iter().enumerate().any(|(index, argument)| {
        let argument = argument.to_string_lossy();
        if let Some(value) = argument.strip_prefix("--user-data-dir=") {
            return normalized_path_text(Path::new(value.trim_matches('"'))) == expected;
        }
        (argument == "--user-data-dir" || argument == "--profile" || argument == "-profile")
            && arguments.get(index + 1).is_some_and(|value| {
                normalized_path_text(Path::new(value.to_string_lossy().trim_matches('"')))
                    == expected
            })
    })
}

fn normalized_path_text(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = path
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_string();
    if cfg!(windows) {
        text.to_ascii_lowercase()
    } else {
        text
    }
}

fn drain_stderr(child: &mut Child) -> Arc<Mutex<VecDeque<String>>> {
    let tail = Arc::new(Mutex::new(VecDeque::with_capacity(BROWSER_STDERR_LINES)));
    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr).lines();
        let captured = Arc::clone(&tail);
        tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(mut lines) = captured.lock() {
                    if lines.len() == BROWSER_STDERR_LINES {
                        lines.pop_front();
                    }
                    lines.push_back(line);
                }
            }
        });
    }
    tail
}

fn browser_startup_error(message: String, stderr_tail: &Arc<Mutex<VecDeque<String>>>) -> CliError {
    let stderr = stderr_tail
        .lock()
        .ok()
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join(" | "))
        .filter(|value| !value.is_empty());
    let detail = stderr
        .map(|stderr| format!("{message}. Browser stderr: {stderr}"))
        .unwrap_or(message);
    CliError::Config(format!(
        "{detail}. Check that Chrome or Edge can start normally, or set SUNOX_BROWSER_PATH."
    ))
}

fn urlencode(s: &str) -> String {
    s.replace(":", "%3A").replace("/", "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn owned_cdp_port_comes_from_profile_file() {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        writeln!(file, "43123\n/devtools/browser/owned").expect("write port");

        assert_eq!(read_owned_cdp_port(file.path()).expect("port"), 43123);
    }

    #[test]
    fn interactive_cdp_websocket_is_pinned_to_owned_loopback_port() {
        let pinned =
            validate_and_pin_ws_url("ws://localhost:43123/devtools/page/owned-target", 43123)
                .expect("owned endpoint");

        assert_eq!(pinned, "ws://127.0.0.1:43123/devtools/page/owned-target");
        assert!(validate_and_pin_ws_url("ws://example.com:43123/devtools/page/a", 43123).is_err());
        assert!(validate_and_pin_ws_url("ws://127.0.0.1:43124/devtools/page/a", 43123).is_err());
    }

    #[test]
    fn browser_process_matching_accepts_equals_and_split_profile_arguments() {
        let profile = Path::new(r"C:\Users\alice\AppData\Local\sunox\profile");
        let equals = vec![
            "chrome.exe".into(),
            r"--user-data-dir=C:\Users\alice\AppData\Local\sunox\profile".into(),
        ];
        let split = vec![
            "chrome.exe".into(),
            "--user-data-dir".into(),
            r"C:\Users\alice\AppData\Local\sunox\profile".into(),
        ];

        assert!(process_uses_profile(&equals, profile));
        assert!(process_uses_profile(&split, profile));
        assert!(!process_uses_profile(
            &["chrome.exe".into(), "--user-data-dir=C:\\other".into()],
            profile
        ));
    }

    #[test]
    fn manual_browser_exit_is_accepted_after_it_was_seen() {
        assert!(manual_browser_has_closed(true, true, true));
        assert!(!manual_browser_has_closed(false, true, true));
        assert!(!manual_browser_has_closed(true, false, true));
        assert!(!manual_browser_has_closed(true, true, false));
    }

    #[test]
    fn pending_and_transient_auth_errors_keep_interactive_login_open() {
        assert!(interactive_login_should_retry(&CliError::Api {
            code: "no_session",
            message: "no active session".into(),
        }));
        assert!(interactive_login_should_retry(&CliError::Api {
            code: "clerk_exchange_rejected",
            message: "anonymous client".into(),
        }));
        assert!(interactive_login_should_retry(&CliError::Http(
            reqwest::Client::new()
                .get("http://[::1")
                .build()
                .expect_err("invalid URL")
        )));
        assert!(!interactive_login_should_retry(&CliError::Config(
            "invalid browser configuration".into()
        )));
        assert!(interactive_login_should_retry(&CliError::SunoApi {
            code: "api_error",
            status: 503,
            message: "temporary outage".into(),
            retryable: None,
            details: None,
        }));
    }

    #[test]
    fn existing_session_probe_does_not_retry_rejected_credentials() {
        assert!(!transient_login_error(&CliError::Api {
            code: "clerk_exchange_rejected",
            message: "anonymous client".into(),
        }));
        assert!(transient_login_error(&CliError::Api {
            code: "clerk_exchange_failed",
            message: "temporary upstream failure".into(),
        }));
        assert!(transient_login_error(&CliError::SunoApi {
            code: "api_error",
            status: 503,
            message: "temporary outage".into(),
            retryable: None,
            details: None,
        }));
    }

    #[test]
    fn cdp_cookies_prefer_auth_domain_clerk_cookie() {
        let auth = browser_auth_from_cdp_cookies(
            vec![
                CdpCookie {
                    name: "__client".into(),
                    value: "suno-client".into(),
                    domain: ".suno.com".into(),
                },
                CdpCookie {
                    name: "__client".into(),
                    value: "auth-client".into(),
                    domain: "auth.suno.com".into(),
                },
                CdpCookie {
                    name: "ajs_anonymous_id".into(),
                    value: "%22device-123%22".into(),
                    domain: ".suno.com".into(),
                },
            ],
            Some(BrowserEnvironment {
                browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
                user_agent: Some("Mozilla/5.0 Test".into()),
                accept_language: Some("en-US,en;q=0.9".into()),
                client_hints: None,
            }),
        )
        .expect("auth");

        assert_eq!(auth.clerk_client_cookie, "auth-client");
        assert_eq!(auth.device_id.as_deref(), Some("device-123"));
        assert!(auth.cookie_header.contains("__client=auth-client"));
        assert!(!auth.cookie_header.contains("__client=suno-client"));
        assert!(
            auth.cookie_header
                .contains("ajs_anonymous_id=%22device-123%22")
        );
        assert_eq!(
            auth.browser_environment
                .as_ref()
                .and_then(|env| env.user_agent.as_deref()),
            Some("Mozilla/5.0 Test")
        );
    }

    #[test]
    fn legacy_roaming_profile_is_moved_without_losing_files() {
        let temp = tempfile::tempdir().expect("temp dir");
        let roaming = temp.path().join("Roaming").join("profile");
        let local = temp.path().join("Local").join("profile");
        std::fs::create_dir_all(&roaming).expect("roaming profile");
        std::fs::write(roaming.join("Cookies"), "session").expect("profile file");

        migrate_legacy_profile(&roaming, &local).expect("migrate profile");

        assert!(!roaming.exists());
        assert_eq!(
            std::fs::read_to_string(local.join("Cookies")).expect("migrated file"),
            "session"
        );
    }

    #[test]
    fn cross_volume_profile_copy_is_staged_and_verified() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source = temp.path().join("Roaming").join("profile");
        let destination = temp.path().join("Local").join("profile");
        std::fs::create_dir_all(source.join("Default").join("Network")).expect("source profile");
        std::fs::create_dir_all(destination.parent().expect("destination parent"))
            .expect("local parent");
        std::fs::write(source.join("Local State"), "state").expect("local state");
        std::fs::write(
            source.join("Default").join("Network").join("Cookies"),
            "cookies",
        )
        .expect("cookies");

        copy_profile_across_volumes(&source, &destination).expect("copy profile");

        assert_eq!(
            profile_tree_stats(&source).expect("source stats"),
            profile_tree_stats(&destination).expect("destination stats")
        );
        assert!(source.exists(), "source is removed only after commit");
        assert!(destination.exists());
    }

    #[test]
    fn dedicated_profile_rejects_a_different_browser_binary() {
        let temp = tempfile::tempdir().expect("temp dir");
        let profile = temp.path().join("profile");
        let chrome = temp.path().join("chrome.exe");
        let edge = temp.path().join("msedge.exe");
        std::fs::create_dir_all(&profile).expect("profile");
        std::fs::write(&chrome, "").expect("chrome fixture");
        std::fs::write(&edge, "").expect("edge fixture");

        validate_profile_browser_identity(&profile, &chrome).expect("unbound profile");
        assert!(!profile.join("sunox-browser-path.txt").exists());
        record_profile_browser_identity(&profile, &chrome).expect("record browser");
        let error = validate_profile_browser_identity(&profile, &edge)
            .expect_err("different browser must be rejected");

        assert!(error.to_string().contains("sunox logout"));
    }

    #[test]
    fn cdp_cookies_ignore_non_suno_domains() {
        let auth = browser_auth_from_cdp_cookies(
            vec![
                CdpCookie {
                    name: "__client".into(),
                    value: "wrong-client".into(),
                    domain: "example.com".into(),
                },
                CdpCookie {
                    name: "sid".into(),
                    value: "session".into(),
                    domain: ".suno.com".into(),
                },
            ],
            None,
        );

        assert!(auth.is_none());
    }

    #[test]
    fn browser_languages_become_accept_language_header() {
        assert_eq!(
            accept_language_from_browser_languages(&[
                "en-US".to_string(),
                "en".to_string(),
                "ja".to_string(),
            ])
            .as_deref(),
            Some("en-US,en;q=0.9,ja;q=0.8")
        );
    }

    #[test]
    fn cdp_version_user_agent_becomes_browser_environment() {
        let environment = browser_environment_from_cdp_version_value(&serde_json::json!({
            "Browser": "Chrome/146.0.0.0",
            "User-Agent": "Mozilla/5.0 RealBrowser"
        }))
        .expect("environment");

        assert_eq!(
            environment.browser_source.as_deref(),
            Some(INTERACTIVE_BROWSER_SOURCE)
        );
        assert_eq!(
            environment.user_agent.as_deref(),
            Some("Mozilla/5.0 RealBrowser")
        );
        assert_eq!(environment.accept_language, None);
    }

    #[test]
    fn metadata_probe_does_not_persist_headless_only_user_agent() {
        assert_eq!(
            normalize_runtime_user_agent(
                "Mozilla/5.0 (Macintosh) AppleWebKit/537.36 HeadlessChrome/150.0.0.0 Safari/537.36"
            ),
            "Mozilla/5.0 (Macintosh) AppleWebKit/537.36 Chrome/150.0.0.0 Safari/537.36"
        );
    }

    #[tokio::test]
    #[ignore = "requires an installed local Chromium-family browser"]
    async fn installed_browser_metadata_probe_returns_public_runtime_values() {
        let source = crate::browser::installed_chromium_browser_sources()
            .into_iter()
            .next()
            .expect("installed Chromium-family browser");

        let environment = probe_browser_runtime_environment(&source)
            .await
            .expect("runtime browser environment");

        assert_eq!(environment.browser_source.as_deref(), Some(source.as_str()));
        assert!(
            environment
                .user_agent
                .as_deref()
                .is_some_and(|value| value.contains("Mozilla/5.0") && !value.contains("Headless"))
        );
        assert!(environment.accept_language.is_some());
        let hints = environment.client_hints.expect("captured client hints");
        assert!(hints.sec_ch_ua.contains("Chromium"));
        assert_eq!(hints.sec_ch_ua_mobile, "?0");
        assert!(!hints.sec_ch_ua_platform.is_empty());
    }

    #[test]
    fn runtime_user_agent_data_becomes_exact_client_hint_headers() {
        let hints = browser_client_hints_from_probe(UserAgentDataProbe {
            brands: vec![
                UserAgentBrandProbe {
                    brand: "Not_A Brand".into(),
                    version: "99".into(),
                },
                UserAgentBrandProbe {
                    brand: "Chromium".into(),
                    version: "150".into(),
                },
            ],
            mobile: false,
            platform: "macOS".into(),
        })
        .expect("client hints");

        assert_eq!(
            hints.sec_ch_ua,
            r#""Not_A Brand";v="99", "Chromium";v="150""#
        );
        assert_eq!(hints.sec_ch_ua_mobile, "?0");
        assert_eq!(hints.sec_ch_ua_platform, r#""macOS""#);
    }

    #[test]
    fn firefox_bidi_session_uses_the_runtime_user_agent() {
        let environment = firefox_environment_from_session_new(
            &serde_json::json!({
                "capabilities": {
                    "userAgent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:151.0) Gecko/20100101 Firefox/151.0"
                }
            }),
            "firefox-developer",
        )
        .expect("Firefox runtime environment");

        assert_eq!(
            environment.user_agent.as_deref(),
            Some(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:151.0) Gecko/20100101 Firefox/151.0"
            )
        );
        assert_eq!(
            environment.browser_source.as_deref(),
            Some("firefox-developer")
        );
        assert_eq!(environment.client_hints, None);
    }

    #[test]
    fn firefox_bidi_port_is_read_only_from_the_owned_process_announcement() {
        let lines = VecDeque::from([
            "noise on stderr".into(),
            "WebDriver BiDi listening on ws://127.0.0.1:49152".into(),
        ]);
        assert_eq!(firefox_bidi_port(&lines), Some(49152));
        assert_eq!(
            firefox_bidi_port(&VecDeque::from([
                "WebDriver BiDi listening on ws://example.com:49152".into()
            ])),
            None
        );
    }

    #[test]
    fn browser_environment_merge_falls_back_by_field() {
        let merged = merge_browser_environments(
            Some(BrowserEnvironment {
                browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
                user_agent: None,
                accept_language: Some("ja,en;q=0.9".into()),
                client_hints: None,
            }),
            Some(BrowserEnvironment {
                browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
                user_agent: Some("Mozilla/5.0 VersionFallback".into()),
                accept_language: None,
                client_hints: None,
            }),
        )
        .expect("environment");

        assert_eq!(
            merged.user_agent.as_deref(),
            Some("Mozilla/5.0 VersionFallback")
        );
        assert_eq!(merged.accept_language.as_deref(), Some("ja,en;q=0.9"));
    }

    #[test]
    fn delete_profile_dir_removes_nested_profile_files() {
        let profile_dir = std::env::temp_dir().join(format!(
            "sunox-profile-delete-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(profile_dir.join("Default")).expect("profile dir");
        std::fs::write(profile_dir.join("Default").join("Cookies"), "cookie db")
            .expect("cookie db");

        delete_interactive_browser_profile_at(&profile_dir).expect("delete profile");

        assert!(!profile_dir.exists());
    }
}
