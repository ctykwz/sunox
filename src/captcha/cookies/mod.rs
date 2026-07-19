//! Cookie sources and conversion for the browser-backed captcha solver.

use std::collections::HashSet;

use crate::auth::AuthState;
use crate::core::CliError;

#[cfg(not(target_os = "windows"))]
mod browser;
mod cdp_cookie;

#[cfg(not(target_os = "windows"))]
use browser::add_live_browser_cookies;
use cdp_cookie::{CdpCookie, add_minimal_cookies_from_header, push_cookie};

pub(super) fn extract_cookies(auth: &AuthState) -> Result<Vec<CdpCookie>, CliError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    // Persisted cookies belong to the verified account. Add them first so a
    // different installed browser cannot replace them merely because its
    // cookie database appears earlier in a discovery list.
    add_stored_auth_cookies(auth, &mut out, &mut seen);
    // Chromium's Windows App-Bound encryption makes live cookie reads both
    // unreliable and potentially disruptive while the user's browser is open.
    // The verified stored values remain the authoritative Windows source.
    #[cfg(not(target_os = "windows"))]
    add_live_browser_cookies(
        auth.browser_environment
            .as_ref()
            .and_then(|environment| environment.browser_source.as_deref()),
        &mut out,
        &mut seen,
    );

    Ok(out)
}

fn add_stored_auth_cookies(
    auth: &AuthState,
    out: &mut Vec<CdpCookie>,
    seen: &mut HashSet<(String, String)>,
) {
    if let Some(clerk) = auth
        .clerk_client_cookie
        .as_deref()
        .filter(|cookie| !cookie.trim().is_empty())
    {
        push_cookie(out, seen, "__client", clerk.trim(), "auth.suno.com", true);
        push_cookie(out, seen, "__client", clerk.trim(), ".suno.com", true);
    }

    if let Some(device_id) = auth
        .device_id
        .as_deref()
        .filter(|device_id| !device_id.trim().is_empty())
    {
        push_cookie(
            out,
            seen,
            "ajs_anonymous_id",
            device_id.trim(),
            ".suno.com",
            false,
        );
    }

    if let Some(cookie_header) = auth
        .cookie
        .as_deref()
        .filter(|cookie| !cookie.trim().is_empty())
    {
        add_minimal_cookies_from_header(cookie_header, out, seen);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::auth::AuthState;

    use super::cdp_cookie::push_cookie;
    use super::{CdpCookie, add_stored_auth_cookies};

    #[test]
    fn stored_clerk_cookie_is_available_for_challenge_browser() {
        let auth = AuthState {
            clerk_client_cookie: Some("stored-client-token".to_string()),
            ..Default::default()
        };
        let mut cookies: Vec<CdpCookie> = Vec::new();
        let mut seen = HashSet::new();
        push_cookie(
            &mut cookies,
            &mut seen,
            "statsig_stable_id",
            "live-statsig",
            ".suno.com",
            false,
        );

        add_stored_auth_cookies(&auth, &mut cookies, &mut seen);

        let cookies = serde_json::to_value(&cookies).expect("cookies serialize");
        let cookies = cookies.as_array().expect("cookie array");
        assert!(cookies.iter().any(|cookie| {
            cookie["name"] == "__client"
                && cookie["value"] == "stored-client-token"
                && cookie["domain"] == "auth.suno.com"
        }));
        assert!(cookies.iter().any(|cookie| {
            cookie["name"] == "__client"
                && cookie["value"] == "stored-client-token"
                && cookie["domain"] == ".suno.com"
        }));
    }

    #[test]
    fn stored_account_cookie_wins_over_later_browser_cookie() {
        let auth = AuthState {
            clerk_client_cookie: Some("stored-client-token".to_string()),
            ..Default::default()
        };
        let mut cookies: Vec<CdpCookie> = Vec::new();
        let mut seen = HashSet::new();
        add_stored_auth_cookies(&auth, &mut cookies, &mut seen);
        push_cookie(
            &mut cookies,
            &mut seen,
            "__client",
            "different-browser-token",
            ".suno.com",
            true,
        );

        let cookies = serde_json::to_value(&cookies).expect("cookies serialize");
        assert!(
            cookies
                .as_array()
                .expect("cookie array")
                .iter()
                .any(|cookie| {
                    cookie["name"] == "__client"
                        && cookie["value"] == "stored-client-token"
                        && cookie["domain"] == ".suno.com"
                })
        );
        assert!(!cookies.to_string().contains("different-browser-token"));
    }
}
