use std::collections::HashSet;

use serde::Serialize;

use crate::auth::is_suno_auth_cookie_domain;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::captcha) struct CdpCookie {
    name: String,
    value: String,
    domain: String,
    path: String,
    secure: bool,
    http_only: bool,
    same_site: &'static str,
}

pub(super) fn add_minimal_cookies_from_header(
    cookie_header: &str,
    out: &mut Vec<CdpCookie>,
    seen: &mut HashSet<(String, String)>,
) {
    for part in cookie_header.split(';') {
        let Some((name, value)) = part.trim().split_once('=') else {
            continue;
        };
        add_minimal_cookie(name.trim(), value.trim(), ".suno.com", false, out, seen);
    }
}

pub(super) fn add_minimal_cookie(
    name: &str,
    value: &str,
    domain: &str,
    http_only: bool,
    out: &mut Vec<CdpCookie>,
    seen: &mut HashSet<(String, String)>,
) {
    if name.is_empty() || value.is_empty() || !is_captcha_cookie(name) {
        return;
    }

    if name == "__client" || name.starts_with("__client_") {
        push_cookie(out, seen, name, value, "auth.suno.com", true);
        push_cookie(out, seen, name, value, ".suno.com", true);
        return;
    }

    let cookie_domain = if is_suno_auth_cookie_domain(domain) {
        "auth.suno.com"
    } else {
        ".suno.com"
    };
    push_cookie(out, seen, name, value, cookie_domain, http_only);
}

pub(super) fn push_cookie(
    out: &mut Vec<CdpCookie>,
    seen: &mut HashSet<(String, String)>,
    name: &str,
    value: &str,
    domain: &str,
    http_only: bool,
) {
    let key = (name.to_string(), domain.to_string());
    if !seen.insert(key) {
        return;
    }
    out.push(CdpCookie {
        name: name.to_string(),
        value: value.to_string(),
        domain: domain.to_string(),
        path: "/".to_string(),
        secure: true,
        http_only,
        same_site: "Lax",
    });
}

fn is_captcha_cookie(name: &str) -> bool {
    matches!(
        name,
        "__client"
            | "__session"
            | "clerk_active_context"
            | "ajs_anonymous_id"
            | "suno_device_id"
            | "statsig_stable_id"
            | "ssr_bucket"
            | "has_logged_in_before"
    ) || name.starts_with("__client_")
        || name.starts_with("__session_")
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::add_minimal_cookie;

    #[test]
    fn bare_clerk_client_cookie_is_injected_for_auth_and_suno_domains() {
        let mut cookies = Vec::new();
        let mut seen = HashSet::new();

        add_minimal_cookie(
            "__client",
            "client-token",
            ".suno.com",
            true,
            &mut cookies,
            &mut seen,
        );

        assert_eq!(cookies.len(), 2);
        assert!(cookies.iter().any(|cookie| {
            cookie.name == "__client"
                && cookie.value == "client-token"
                && cookie.domain == "auth.suno.com"
        }));
        assert!(cookies.iter().any(|cookie| {
            cookie.name == "__client"
                && cookie.value == "client-token"
                && cookie.domain == ".suno.com"
        }));
    }
}
