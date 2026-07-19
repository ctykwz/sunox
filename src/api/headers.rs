use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue, USER_AGENT};

use super::SunoClient;
use crate::auth;
use crate::net::http;

impl SunoClient {
    pub(crate) fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let (jwt, device, browser_environment) = {
            let auth = self.auth.lock().expect("auth mutex poisoned");
            (
                auth.jwt.clone(),
                auth.device_id.clone(),
                auth.browser_environment.clone(),
            )
        };

        if let Some(jwt) = jwt
            && let Ok(val) = format!("Bearer {jwt}").parse()
        {
            headers.insert("authorization", val);
        }
        if let Some(device) = device.as_deref().filter(|value| !value.trim().is_empty())
            && let Ok(val) = device.parse()
        {
            headers.insert("device-id", val);
        }
        if let Ok(val) = "*/*".parse() {
            headers.insert(ACCEPT, val);
        }
        if let Ok(val) = auth::browser_token().parse() {
            headers.insert("browser-token", val);
        }
        headers.extend(browser_environment_headers(browser_environment.as_ref()));
        if let Ok(val) = "https://suno.com".parse() {
            headers.insert("origin", val);
        }
        if let Ok(val) = "https://suno.com/".parse() {
            headers.insert("referer", val);
        }
        headers
    }
}

pub(crate) fn browser_environment_headers(
    browser_environment: Option<&crate::auth::BrowserEnvironment>,
) -> HeaderMap {
    let mut headers = HeaderMap::new();
    let user_agent = browser_environment
        .and_then(|env| env.user_agent.as_deref())
        .filter(|value| HeaderValue::from_str(value).is_ok())
        .unwrap_or(http::BROWSER_USER_AGENT);
    if let Ok(val) = user_agent.parse() {
        headers.insert(USER_AGENT, val);
    }
    let accept_language = browser_environment
        .and_then(|env| env.accept_language.as_deref())
        .filter(|value| HeaderValue::from_str(value).is_ok())
        .unwrap_or(http::BROWSER_ACCEPT_LANGUAGE);
    if let Ok(val) = accept_language.parse() {
        headers.insert(ACCEPT_LANGUAGE, val);
    }
    let captured_client_hints = browser_environment
        .and_then(|environment| environment.client_hints.as_ref())
        .filter(|hints| {
            HeaderValue::from_str(&hints.sec_ch_ua).is_ok()
                && HeaderValue::from_str(&hints.sec_ch_ua_mobile).is_ok()
                && HeaderValue::from_str(&hints.sec_ch_ua_platform).is_ok()
        })
        .map(|hints| ChromiumClientHints {
            sec_ch_ua: hints.sec_ch_ua.clone(),
            sec_ch_ua_mobile: hints.sec_ch_ua_mobile.clone(),
            sec_ch_ua_platform: hints.sec_ch_ua_platform.clone(),
        });
    if let Some(client_hints) = captured_client_hints.or_else(|| chromium_client_hints(user_agent))
    {
        if let Ok(val) = client_hints.sec_ch_ua.parse() {
            headers.insert("sec-ch-ua", val);
        }
        if let Ok(val) = client_hints.sec_ch_ua_mobile.parse() {
            headers.insert("sec-ch-ua-mobile", val);
        }
        if let Ok(val) = client_hints.sec_ch_ua_platform.parse() {
            headers.insert("sec-ch-ua-platform", val);
        }
    }
    if let Ok(val) = "cors".parse() {
        headers.insert("sec-fetch-mode", val);
    }
    if let Ok(val) = "empty".parse() {
        headers.insert("sec-fetch-dest", val);
    }
    if let Ok(val) = "same-site".parse() {
        headers.insert("sec-fetch-site", val);
    }
    if let Ok(val) = "u=1, i".parse() {
        headers.insert("priority", val);
    }
    headers
}

struct ChromiumClientHints {
    sec_ch_ua: String,
    sec_ch_ua_mobile: String,
    sec_ch_ua_platform: String,
}

fn chromium_client_hints(user_agent: &str) -> Option<ChromiumClientHints> {
    let (brand, major) = if let Some(major) = major_version_after(user_agent, "Edg/") {
        ("Microsoft Edge", major)
    } else {
        ("Google Chrome", major_version_after(user_agent, "Chrome/")?)
    };

    Some(ChromiumClientHints {
        sec_ch_ua: format!(
            r#""{brand}";v="{major}", "Chromium";v="{major}", "Not)A;Brand";v="24""#
        ),
        sec_ch_ua_mobile: if user_agent.contains("Mobile") {
            "?1"
        } else {
            "?0"
        }
        .into(),
        sec_ch_ua_platform: sec_ch_ua_platform(user_agent).into(),
    })
}

fn major_version_after(user_agent: &str, marker: &str) -> Option<String> {
    let version = user_agent.split_once(marker)?.1;
    let end = version
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(version.len());
    if end == 0 {
        None
    } else {
        Some(version[..end].to_string())
    }
}

fn sec_ch_ua_platform(user_agent: &str) -> &'static str {
    if user_agent.contains("Android") {
        r#""Android""#
    } else if user_agent.contains("iPhone") || user_agent.contains("iPad") {
        r#""iOS""#
    } else if user_agent.contains("Windows") {
        r#""Windows""#
    } else if user_agent.contains("Macintosh") || user_agent.contains("Mac OS X") {
        r#""macOS""#
    } else if user_agent.contains("Linux") || user_agent.contains("X11") {
        r#""Linux""#
    } else {
        r#""Unknown""#
    }
}

#[cfg(test)]
mod tests {
    use super::{browser_environment_headers, chromium_client_hints};
    use crate::auth::{BrowserClientHints, BrowserEnvironment};

    #[test]
    fn chrome_user_agent_becomes_current_web_client_hints() {
        let hints = chromium_client_hints(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/149.0.0.0 Safari/537.36",
        )
        .expect("client hints");

        assert_eq!(
            hints.sec_ch_ua,
            r#""Google Chrome";v="149", "Chromium";v="149", "Not)A;Brand";v="24""#
        );
        assert_eq!(hints.sec_ch_ua_mobile, "?0");
        assert_eq!(hints.sec_ch_ua_platform, r#""macOS""#);
    }

    #[test]
    fn edge_user_agent_keeps_edge_brand() {
        let hints =
            chromium_client_hints("Mozilla/5.0 Chrome/149.0.0.0 Safari/537.36 Edg/149.0.0.0")
                .expect("client hints");

        assert_eq!(
            hints.sec_ch_ua,
            r#""Microsoft Edge";v="149", "Chromium";v="149", "Not)A;Brand";v="24""#
        );
    }

    #[test]
    fn windows_user_agent_sets_windows_platform_hint() {
        let hints = chromium_client_hints(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/149.0.0.0 Safari/537.36",
        )
        .expect("client hints");

        assert_eq!(hints.sec_ch_ua_platform, r#""Windows""#);
    }

    #[test]
    fn non_chromium_user_agent_omits_client_hints() {
        assert!(chromium_client_hints("Mozilla/5.0 Firefox/140.0").is_none());
    }

    #[test]
    fn captured_client_hints_override_ua_derived_guesses() {
        let headers = browser_environment_headers(Some(&BrowserEnvironment {
            browser_source: Some("brave".into()),
            user_agent: Some("Mozilla/5.0 Chrome/150.0.0.0 Safari/537.36".into()),
            accept_language: Some("zh-CN,zh;q=0.9".into()),
            client_hints: Some(BrowserClientHints {
                sec_ch_ua: r#""Chromium";v="150", "Brave";v="150""#.into(),
                sec_ch_ua_mobile: "?0".into(),
                sec_ch_ua_platform: r#""macOS""#.into(),
            }),
        }));

        assert_eq!(
            headers
                .get("sec-ch-ua")
                .and_then(|value| value.to_str().ok()),
            Some(r#""Chromium";v="150", "Brave";v="150""#)
        );
    }

    #[test]
    fn invalid_stored_headers_use_built_in_fallbacks() {
        let headers = browser_environment_headers(Some(&BrowserEnvironment {
            browser_source: Some("chrome".into()),
            user_agent: Some("bad\nuser-agent".into()),
            accept_language: Some("bad\naccept-language".into()),
            client_hints: None,
        }));

        assert_eq!(
            headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok()),
            Some(crate::net::http::BROWSER_USER_AGENT)
        );
        assert_eq!(
            headers
                .get("accept-language")
                .and_then(|value| value.to_str().ok()),
            Some(crate::net::http::BROWSER_ACCEPT_LANGUAGE)
        );
    }
}
