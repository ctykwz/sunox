use std::collections::HashSet;

use super::cdp_cookie::{CdpCookie, add_minimal_cookie};
use crate::auth::is_suno_cookie_domain;

pub(super) fn add_live_browser_cookies(
    preferred_source: Option<&str>,
    out: &mut Vec<CdpCookie>,
    seen: &mut HashSet<(String, String)>,
) -> bool {
    let domains: Vec<String> = vec![
        "suno.com".into(),
        "auth.suno.com".into(),
        ".suno.com".into(),
    ];
    let Some((browser_name, raw_cookies)) = browser_source_order(preferred_source)
        .into_iter()
        .find_map(|source| {
            let result = browser_cookies(source, domains.clone());
            match result {
                Ok(cookies)
                    if cookies
                        .iter()
                        .any(|cookie| is_suno_cookie_domain(&cookie.domain)) =>
                {
                    Some((browser_label(source), cookies))
                }
                _ => None,
            }
        })
    else {
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

fn browser_cookies(
    source: &str,
    domains: Vec<String>,
) -> rookie::Result<Vec<rookie::enums::Cookie>> {
    match source {
        "chrome" => rookie::chrome(Some(domains)),
        "arc" => rookie::arc(Some(domains)),
        "brave" => rookie::brave(Some(domains)),
        "firefox" => rookie::firefox(Some(domains)),
        "edge" => rookie::edge(Some(domains)),
        _ => unreachable!("browser source order contains only supported sources"),
    }
}

fn browser_source_order(preferred_source: Option<&str>) -> Vec<&'static str> {
    match preferred_source {
        Some(source) => normalized_cookie_source(source).into_iter().collect(),
        None => vec!["chrome", "arc", "brave", "firefox", "edge"],
    }
}

fn normalized_cookie_source(source: &str) -> Option<&'static str> {
    match source.trim().to_ascii_lowercase().as_str() {
        "chrome" | "chromium" => Some("chrome"),
        "arc" => Some("arc"),
        "brave" => Some("brave"),
        "firefox" => Some("firefox"),
        "edge" => Some("edge"),
        _ => None,
    }
}

fn browser_label(source: &str) -> &'static str {
    match source {
        "chrome" => "Chrome",
        "arc" => "Arc",
        "brave" => "Brave",
        "firefox" => "Firefox",
        "edge" => "Edge",
        _ => "browser",
    }
}

#[cfg(test)]
mod tests {
    use super::browser_source_order;

    #[test]
    fn stored_browser_source_excludes_unrelated_cookie_stores() {
        assert_eq!(browser_source_order(Some("edge")), ["edge"]);
        assert_eq!(browser_source_order(Some("arc")), ["arc"]);
        assert!(browser_source_order(Some("edge-dev")).is_empty());
        assert!(browser_source_order(Some("unknown")).is_empty());
        assert_eq!(browser_source_order(None)[0], "chrome");
    }
}
