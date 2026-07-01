use std::collections::HashSet;

use super::cookie::{is_suno_auth_cookie_domain, is_suno_cookie_domain, sanitize_device_id};
use super::environment::browser_environment_for_cookie_source;
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

    for (display_name, source, result) in [
        ("Chrome", "chrome", rookie::chrome(Some(domains.clone()))),
        ("Arc", "arc", rookie::arc(Some(domains.clone()))),
        ("Brave", "brave", rookie::brave(Some(domains.clone()))),
        ("Firefox", "firefox", rookie::firefox(Some(domains.clone()))),
        ("Edge", "edge", rookie::edge(Some(domains.clone()))),
    ] {
        if let Ok(cookies) = result
            && let Some(auth) = browser_auth_from_cookies(source, cookies)
        {
            eprintln!("Found Suno session in {display_name}");
            return Ok(auth);
        }
    }

    Err(CliError::Config(
        "No Suno session found in any browser. Log into suno.com first, then retry.".into(),
    ))
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
