use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::tungstenite::Message;

use super::cookie::{is_suno_auth_cookie_domain, is_suno_cookie_domain, sanitize_device_id};
use super::environment::{accept_language_from_browser_languages, non_empty_header_value};
use super::types::{BrowserAuth, BrowserEnvironment};
use crate::browser::locate_chromium_browser;
use crate::core::CliError;

const CDP_HOST: &str = "127.0.0.1";
const LOGIN_URL: &str = "https://suno.com/create";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const INTERACTIVE_BROWSER_SOURCE: &str = "interactive-browser";

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

impl CdpSession {
    async fn connect(ws_url: &str) -> Result<Self, CliError> {
        let (ws, _) = tokio_tungstenite::connect_async(ws_url)
            .await
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

pub async fn extract_interactive_browser_auth() -> Result<BrowserAuth, CliError> {
    let port = allocate_cdp_port()?;
    let mut child = spawn_login_browser(port).await?;

    let result = async {
        wait_for_cdp(port).await?;
        let version_environment = browser_environment_from_cdp_version(port).await;
        let ws_url = find_or_create_login_tab(port).await?;
        eprintln!("Complete Suno login in the opened browser window...");
        wait_for_suno_auth(ws_url, version_environment).await
    }
    .await;

    if result.is_ok() {
        eprintln!("Suno login captured; closing the dedicated browser window.");
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    result
}

pub fn delete_interactive_browser_profile() -> Result<(), CliError> {
    let profile_dir = interactive_browser_profile_dir()?;
    delete_interactive_browser_profile_at(&profile_dir)
}

async fn spawn_login_browser(port: u16) -> Result<Child, CliError> {
    let browser_path = locate_chromium_browser()?;
    let profile_dir = interactive_browser_profile_dir()?;
    std::fs::create_dir_all(&profile_dir)?;

    eprintln!(
        "Opening a dedicated browser profile for Suno login. This avoids reading your default browser cookies."
    );

    let mut child = Command::new(&browser_path)
        .arg(format!("--remote-debugging-port={port}"))
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-search-engine-choice-screen")
        .arg("--disable-features=TranslateUI")
        .arg("--window-size=1280,900")
        .arg(LOGIN_URL)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            CliError::Config(format!("failed to spawn browser at {browser_path:?}: {e}"))
        })?;
    drain_stderr(&mut child);
    Ok(child)
}

fn interactive_browser_profile_dir() -> Result<PathBuf, CliError> {
    directories::ProjectDirs::from("com", "sunox", "sunox")
        .map(|d| d.data_dir().join("interactive-login-browser-profile"))
        .ok_or_else(|| CliError::Config("could not resolve data dir for browser profile".into()))
}

fn delete_interactive_browser_profile_at(profile_dir: &Path) -> Result<(), CliError> {
    if profile_dir.exists() {
        std::fs::remove_dir_all(profile_dir)?;
    }
    Ok(())
}

async fn wait_for_cdp(port: u16) -> Result<(), CliError> {
    for _ in 0..30 {
        if cdp_version(port).await.is_ok() {
            return Ok(());
        }
        sleep(Duration::from_millis(500)).await;
    }

    Err(CliError::Config(
        "Browser was spawned but never opened the CDP port. Check that Chrome or Edge can start normally, or set SUNO_BROWSER_PATH to a Chromium-family browser binary.".into(),
    ))
}

async fn cdp_version(port: u16) -> Result<serde_json::Value, CliError> {
    let url = format!("http://{CDP_HOST}:{port}/json/version");
    reqwest::Client::new()
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
            target.web_socket_debugger_url
        } else {
            None
        }
    }) {
        return Ok(ws_url);
    }

    let url = format!("http://{CDP_HOST}:{port}/json/new?{}", urlencode(LOGIN_URL));
    let target: CdpTarget = reqwest::Client::new()
        .put(&url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/new: {e}")))?
        .json()
        .await
        .map_err(|e| CliError::Config(format!("CDP /json/new parse: {e}")))?;
    target
        .web_socket_debugger_url
        .ok_or_else(|| CliError::Config("CDP /json/new did not return a websocket URL".into()))
}

async fn cdp_list(port: u16) -> Result<Vec<CdpTarget>, CliError> {
    let url = format!("http://{CDP_HOST}:{port}/json/list");
    reqwest::Client::new()
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
) -> Result<BrowserAuth, CliError> {
    let deadline = tokio::time::Instant::now() + LOGIN_TIMEOUT;
    let mut session = CdpSession::connect(&ws_url).await?;
    session
        .call("Network.enable", serde_json::json!({}))
        .await?;
    session.call("Page.enable", serde_json::json!({})).await?;
    session
        .call(
            "Page.navigate",
            serde_json::json!({
                "url": LOGIN_URL
            }),
        )
        .await?;
    let browser_environment = merge_browser_environments(
        browser_environment_from_page(&mut session).await,
        fallback_environment,
    )
    .or_else(|| Some(interactive_browser_environment()));

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(CliError::Config(
                "Timed out waiting for Suno login in the dedicated browser window.".into(),
            ));
        }

        let result = session
            .call("Network.getAllCookies", serde_json::json!({}))
            .await?;
        let cookies: Vec<CdpCookie> =
            serde_json::from_value(result.get("cookies").cloned().unwrap_or_default())
                .map_err(|e| CliError::Config(format!("CDP cookie parse: {e}")))?;
        if let Some(auth) = browser_auth_from_cdp_cookies(cookies, browser_environment.clone()) {
            return Ok(auth);
        }
        sleep(POLL_INTERVAL).await;
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
        })
    }
}

async fn browser_environment_from_page(session: &mut CdpSession) -> Option<BrowserEnvironment> {
    let result = session
        .call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "JSON.stringify({ userAgent: navigator.userAgent, languages: Array.from(navigator.languages || [navigator.language]).filter(Boolean) })",
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

    if user_agent.is_none() && accept_language.is_none() {
        None
    } else {
        Some(BrowserEnvironment {
            browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
            user_agent,
            accept_language,
        })
    }
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

fn allocate_cdp_port() -> Result<u16, CliError> {
    for port in 9234..9260 {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        if TcpListener::bind(addr).is_ok() {
            return Ok(port);
        }
    }
    Err(CliError::Config(
        "could not find an available local browser debugging port".into(),
    ))
}

fn drain_stderr(child: &mut Child) {
    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr).lines();
        tokio::spawn(async move {
            while let Ok(Some(_)) = reader.next_line().await {
                // discard browser startup noise.
            }
        });
    }
}

fn urlencode(s: &str) -> String {
    s.replace(":", "%3A").replace("/", "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn browser_environment_merge_falls_back_by_field() {
        let merged = merge_browser_environments(
            Some(BrowserEnvironment {
                browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
                user_agent: None,
                accept_language: Some("ja,en;q=0.9".into()),
            }),
            Some(BrowserEnvironment {
                browser_source: Some(INTERACTIVE_BROWSER_SOURCE.into()),
                user_agent: Some("Mozilla/5.0 VersionFallback".into()),
                accept_language: None,
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
