//! Cookie sources and conversion for the browser-backed captcha solver.

use std::collections::HashSet;

use crate::auth::AuthState;
use crate::core::CliError;

mod browser;
mod cdp_cookie;

use browser::add_live_browser_cookies;
use cdp_cookie::{CdpCookie, add_minimal_cookies_from_header, push_cookie};

pub(super) fn extract_cookies(auth: &AuthState) -> Result<Vec<CdpCookie>, CliError> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    add_live_browser_cookies(&mut out, &mut seen);
    add_stored_auth_cookies(auth, &mut out, &mut seen);

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
    fn stored_clerk_cookie_is_merged_after_partial_live_cookies() {
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
}
