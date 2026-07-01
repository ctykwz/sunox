use std::collections::HashSet;

use super::cdp_cookie::{CdpCookie, add_minimal_cookie};
use crate::auth::is_suno_cookie_domain;

pub(super) fn add_live_browser_cookies(
    out: &mut Vec<CdpCookie>,
    seen: &mut HashSet<(String, String)>,
) -> bool {
    let domains: Vec<String> = vec![
        "suno.com".into(),
        "auth.suno.com".into(),
        ".suno.com".into(),
    ];
    let Some((browser_name, raw_cookies)) = [
        ("Chrome", rookie::chrome(Some(domains.clone()))),
        ("Arc", rookie::arc(Some(domains.clone()))),
        ("Brave", rookie::brave(Some(domains.clone()))),
        ("Firefox", rookie::firefox(Some(domains.clone()))),
        ("Edge", rookie::edge(Some(domains))),
    ]
    .into_iter()
    .find_map(|(browser_name, result)| match result {
        Ok(cookies)
            if cookies
                .iter()
                .any(|cookie| is_suno_cookie_domain(&cookie.domain)) =>
        {
            Some((browser_name, cookies))
        }
        _ => None,
    }) else {
        return false;
    };

    for cookie in raw_cookies {
        if !is_suno_cookie_domain(&cookie.domain) {
            continue;
        }
        add_minimal_cookie(
            &cookie.name,
            &cookie.value,
            &cookie.domain,
            cookie.http_only,
            out,
            seen,
        );
    }

    if !out.is_empty() {
        eprintln!("Using fresh Suno browser cookies from {browser_name}");
    }
    !out.is_empty()
}
