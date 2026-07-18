use std::collections::HashSet;

use super::cookie::{is_suno_auth_cookie_domain, is_suno_cookie_domain, sanitize_device_id};
use super::environment::{browser_environment_for_cookie_source, chromium_user_data_dirs};
use super::types::{BrowserAuth, BrowserEnvironment};
use crate::core::CliError;

/// Extract Suno auth cookies from the user's browsers.
/// Tries Chrome, Arc, Brave, Firefox, and Edge in order.
pub fn extract_browser_auth() -> Result<BrowserAuth, CliError> {
    let domains = vec![
        "suno.com".into(),
        "auth.suno.com".into(),
        ".suno.com".into(),
    ];

    let mut diagnostics = Vec::new();

    #[cfg(windows)]
    for (display_name, source) in [("Chrome", "chrome"), ("Brave", "brave"), ("Edge", "edge")] {
        if let Some(auth) =
            probe_windows_chromium_profiles(display_name, source, &domains, &mut diagnostics)
        {
            return Ok(auth);
        }
    }

    #[cfg(not(windows))]
    {
        if let Some(auth) = record_probe(
            "Chrome",
            "chrome",
            rookie::chrome(Some(domains.clone())),
            &mut diagnostics,
        ) {
            return Ok(auth);
        }
        if let Some(auth) = record_probe(
            "Brave",
            "brave",
            rookie::brave(Some(domains.clone())),
            &mut diagnostics,
        ) {
            return Ok(auth);
        }
        if let Some(auth) = record_probe(
            "Edge",
            "edge",
            rookie::edge(Some(domains.clone())),
            &mut diagnostics,
        ) {
            return Ok(auth);
        }
    }

    if let Some(auth) = record_probe(
        "Arc",
        "arc",
        rookie::arc(Some(domains.clone())),
        &mut diagnostics,
    ) {
        return Ok(auth);
    }
    if let Some(auth) = record_probe(
        "Firefox",
        "firefox",
        rookie::firefox(Some(domains)),
        &mut diagnostics,
    ) {
        return Ok(auth);
    }

    Err(CliError::Config(format!(
        "No Suno session found in any browser. Log into suno.com first, then retry. Browser probes: {}",
        diagnostics.join("; ")
    )))
}

fn record_probe<E: std::fmt::Display>(
    display_name: &str,
    source: &str,
    result: Result<Vec<rookie::enums::Cookie>, E>,
    diagnostics: &mut Vec<String>,
) -> Option<BrowserAuth> {
    match result {
        Ok(cookies) => {
            let cookie_count = cookies.len();
            let auth = browser_auth_from_cookies(source, cookies);
            if auth.is_some() {
                eprintln!("Found Suno session in {display_name}");
            } else {
                diagnostics.push(format!(
                    "{display_name}: read {cookie_count} matching cookies but found no usable __client"
                ));
            }
            auth
        }
        Err(error) => {
            diagnostics.push(format!("{display_name}: {error}"));
            None
        }
    }
}

#[cfg(windows)]
fn probe_windows_chromium_profiles(
    display_name: &str,
    source: &str,
    domains: &[String],
    diagnostics: &mut Vec<String>,
) -> Option<BrowserAuth> {
    let mut found_database = false;
    for user_data_dir in chromium_user_data_dirs(source) {
        let key_path = user_data_dir.join("Local State");
        let Ok(entries) = std::fs::read_dir(&user_data_dir) else {
            continue;
        };
        let mut profiles = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .filter(|profile| profile_cookie_path(profile).is_some())
            .collect::<Vec<_>>();
        profiles.sort_by_key(|path| path.file_name().map(ToOwned::to_owned));

        for profile in profiles {
            let Some(cookie_path) = profile_cookie_path(&profile) else {
                continue;
            };
            found_database = true;
            let profile_name = profile
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown profile".into());
            let Some(cookie_path) = cookie_path.to_str() else {
                diagnostics.push(format!(
                    "{display_name} {profile_name}: cookie path is not valid Unicode"
                ));
                continue;
            };
            let Some(key_path) = key_path.to_str() else {
                diagnostics.push(format!(
                    "{display_name} {profile_name}: Local State path is not valid Unicode"
                ));
                continue;
            };
            if let Some(auth) = record_probe(
                &format!("{display_name} ({profile_name})"),
                source,
                rookie::any_browser(cookie_path, Some(domains.to_vec()), Some(key_path)),
                diagnostics,
            ) {
                return Some(auth);
            }
        }
    }

    if !found_database {
        diagnostics.push(format!("{display_name}: no profile cookie database found"));
    }
    None
}

#[cfg(windows)]
fn profile_cookie_path(profile: &std::path::Path) -> Option<std::path::PathBuf> {
    [
        profile.join("Network").join("Cookies"),
        profile.join("Cookies"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn browser_auth_from_cookies(
    browser_source: &str,
    cookies: Vec<rookie::enums::Cookie>,
) -> Option<BrowserAuth> {
    browser_auth_from_cookies_with_environment(
        browser_environment_for_cookie_source(browser_source),
        cookies,
    )
}

fn browser_auth_from_cookies_with_environment(
    browser_environment: BrowserEnvironment,
    cookies: Vec<rookie::enums::Cookie>,
) -> Option<BrowserAuth> {
    let mut seen = HashSet::new();
    let mut header_parts = Vec::new();
    let mut clerk_client_cookie: Option<String> = None;
    let mut auth_domain_clerk: Option<String> = None;
    let mut device_id: Option<String> = None;

    for cookie in cookies {
        if !is_suno_cookie_domain(&cookie.domain) {
            continue;
        }
        if cookie.name == "__client" && !cookie.value.is_empty() {
            if is_suno_auth_cookie_domain(&cookie.domain) {
                auth_domain_clerk = Some(cookie.value.clone());
            } else if clerk_client_cookie.is_none() {
                clerk_client_cookie = Some(cookie.value.clone());
            }
        }
        if cookie.name == "ajs_anonymous_id" && device_id.is_none() {
            device_id = sanitize_device_id(&cookie.value);
        }
        let key = (cookie.name.clone(), cookie.domain.clone());
        if seen.insert(key) {
            header_parts.push(format!("{}={}", cookie.name, cookie.value));
        }
    }

    let clerk_client_cookie = auth_domain_clerk.or(clerk_client_cookie)?;
    Some(BrowserAuth {
        clerk_client_cookie,
        cookie_header: header_parts.join("; "),
        device_id,
        browser_environment: Some(browser_environment),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rookie_cookie(name: &str, value: &str, domain: &str) -> rookie::enums::Cookie {
        rookie::enums::Cookie {
            domain: domain.into(),
            path: "/".into(),
            secure: true,
            expires: None,
            name: name.into(),
            value: value.into(),
            http_only: true,
            same_site: 0,
        }
    }

    #[cfg(windows)]
    #[test]
    fn custom_named_chromium_profile_cookie_database_is_detected() {
        let temp = tempfile::tempdir().expect("temp dir");
        let profile = temp.path().join("Work Account");
        std::fs::create_dir_all(profile.join("Network")).expect("profile dir");
        std::fs::write(profile.join("Network").join("Cookies"), "fixture").expect("cookie fixture");

        assert_eq!(
            profile_cookie_path(&profile),
            Some(profile.join("Network").join("Cookies"))
        );
    }

    #[test]
    fn extracted_browser_auth_keeps_partial_browser_environment() {
        let auth = browser_auth_from_cookies_with_environment(
            BrowserEnvironment {
                browser_source: Some("edge".into()),
                user_agent: None,
                accept_language: None,
            },
            vec![
                rookie_cookie("__client", "suno-client", ".suno.com"),
                rookie_cookie("__client", "auth-client", "auth.suno.com"),
                rookie_cookie("ajs_anonymous_id", "%22device-123%22", ".suno.com"),
            ],
        )
        .expect("auth");

        let environment = auth.browser_environment.expect("browser environment");
        assert_eq!(environment.browser_source.as_deref(), Some("edge"));
        assert_eq!(environment.user_agent, None);
        assert_eq!(environment.accept_language, None);
    }

    #[test]
    fn extracted_browser_auth_preserves_available_public_browser_parameters() {
        let auth = browser_auth_from_cookies_with_environment(
            BrowserEnvironment {
                browser_source: Some("edge".into()),
                user_agent: None,
                accept_language: Some("zh-CN,zh;q=0.9".into()),
            },
            vec![
                rookie_cookie("__client", "auth-client", "auth.suno.com"),
                rookie_cookie("ajs_anonymous_id", "%22device-123%22", ".suno.com"),
            ],
        )
        .expect("auth");

        let environment = auth.browser_environment.expect("browser environment");
        assert_eq!(environment.browser_source.as_deref(), Some("edge"));
        assert_eq!(environment.user_agent, None);
        assert_eq!(
            environment.accept_language.as_deref(),
            Some("zh-CN,zh;q=0.9")
        );
    }
}
