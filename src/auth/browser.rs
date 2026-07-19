use std::collections::HashSet;

use super::cookie::{is_suno_auth_cookie_domain, is_suno_cookie_domain, sanitize_device_id};
use super::environment::{
    browser_environment_for_profile, browser_source_for_firefox_profile,
    browser_source_for_profile_root, chromium_user_data_dirs, firefox_profile_dirs,
};
use super::types::{BrowserAuth, BrowserEnvironment};
use crate::core::CliError;

/// Extract Suno auth cookies from the user's browsers.
/// Tries Chrome, Arc, Brave, Firefox, and Edge in order.
pub fn extract_browser_auth() -> Result<BrowserAuth, CliError> {
    extract_browser_auth_matching(None)
}

pub(crate) fn extract_browser_auth_for_clerk(
    clerk_client_cookie: &str,
) -> Result<BrowserAuth, CliError> {
    extract_browser_auth_matching(Some(clerk_client_cookie))
}

fn extract_browser_auth_matching(
    expected_clerk_client_cookie: Option<&str>,
) -> Result<BrowserAuth, CliError> {
    let domains: Vec<String> = vec![
        "suno.com".into(),
        "auth.suno.com".into(),
        ".suno.com".into(),
    ];

    let mut diagnostics = Vec::new();

    #[cfg(not(windows))]
    for (display_name, source) in [
        ("Chrome", "chrome"),
        ("Brave", "brave"),
        ("Edge", "edge"),
        ("Arc", "arc"),
    ] {
        if let Some(auth) = probe_chromium_profiles(
            display_name,
            source,
            &domains,
            expected_clerk_client_cookie,
            &mut diagnostics,
        ) {
            return Ok(auth);
        }
    }

    if let Some(auth) =
        probe_firefox_profiles("Firefox", expected_clerk_client_cookie, &mut diagnostics)
    {
        return Ok(auth);
    }

    Err(CliError::Config(format!(
        "No Suno session found in any browser. Log into suno.com first, then retry. Browser probes: {}",
        diagnostics.join("; ")
    )))
}

fn record_probe_with_environment<E: std::fmt::Display>(
    display_name: &str,
    result: Result<Vec<rookie::enums::Cookie>, E>,
    browser_environment: BrowserEnvironment,
    expected_clerk_client_cookie: Option<&str>,
    diagnostics: &mut Vec<String>,
) -> Option<BrowserAuth> {
    match result {
        Ok(cookies) => {
            let cookie_count = cookies.len();
            let auth = browser_auth_from_cookies_with_environment(
                browser_environment,
                cookies,
                expected_clerk_client_cookie,
            );
            if auth.is_some() {
                eprintln!("Found Suno session in {display_name}");
            } else if expected_clerk_client_cookie.is_some() {
                diagnostics.push(format!(
                    "{display_name}: no Suno session matched the stored account"
                ));
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

fn probe_chromium_profiles(
    display_name: &str,
    source: &str,
    domains: &[String],
    expected_clerk_client_cookie: Option<&str>,
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
            let profile_source = browser_source_for_profile_root(source, &user_data_dir);
            let environment = browser_environment_for_profile(&profile_source, &profile);
            if let Some(auth) = record_probe_with_environment(
                &format!("{display_name} ({profile_name})"),
                chromium_profile_cookies(&profile_source, cookie_path, key_path, domains),
                environment,
                expected_clerk_client_cookie,
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

#[cfg(unix)]
fn chromium_profile_cookies(
    source: &str,
    cookie_path: &str,
    _key_path: &str,
    domains: &[String],
) -> rookie::Result<Vec<rookie::enums::Cookie>> {
    let config_source = match source {
        "chrome" | "chrome-beta" | "chrome-dev" | "chrome-canary" => "chrome",
        "edge" | "edge-beta" | "edge-dev" | "edge-canary" => "edge",
        "brave" | "brave-beta" | "brave-nightly" => "brave",
        "chromium" => "chromium",
        "arc" => "arc",
        other => other,
    };
    rookie::chromium_based(
        rookie::config::get_browser_config(config_source),
        cookie_path.into(),
        Some(domains.to_vec()),
    )
}

#[cfg(windows)]
fn chromium_profile_cookies(
    _source: &str,
    cookie_path: &str,
    key_path: &str,
    domains: &[String],
) -> rookie::Result<Vec<rookie::enums::Cookie>> {
    rookie::any_browser(cookie_path, Some(domains.to_vec()), Some(key_path))
}

fn profile_cookie_path(profile: &std::path::Path) -> Option<std::path::PathBuf> {
    [
        profile.join("Network").join("Cookies"),
        profile.join("Cookies"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn probe_firefox_profiles(
    display_name: &str,
    expected_clerk_client_cookie: Option<&str>,
    diagnostics: &mut Vec<String>,
) -> Option<BrowserAuth> {
    let mut found_database = false;
    for profile_root in firefox_profile_dirs() {
        let Ok(entries) = std::fs::read_dir(profile_root) else {
            continue;
        };
        let mut profiles = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .map(|entry| entry.path())
            .filter(|profile| profile.join("cookies.sqlite").is_file())
            .collect::<Vec<_>>();
        profiles.sort_by_key(|path| path.file_name().map(ToOwned::to_owned));
        for profile in profiles {
            found_database = true;
            let profile_name = profile
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "unknown profile".into());
            let cookies_path = profile.join("cookies.sqlite");
            let Some(cookies_path) = cookies_path.to_str() else {
                diagnostics.push(format!(
                    "{display_name} {profile_name}: cookie path is not valid Unicode"
                ));
                continue;
            };
            let profile_source = browser_source_for_firefox_profile(&profile);
            let environment = browser_environment_for_profile(&profile_source, &profile);
            if let Some(auth) = record_probe_with_environment(
                &format!("{display_name} ({profile_name})"),
                firefox_profile_cookies(cookies_path),
                environment,
                expected_clerk_client_cookie,
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

fn firefox_profile_cookies(cookies_path: &str) -> Result<Vec<rookie::enums::Cookie>, CliError> {
    use rusqlite::{Connection, OpenFlags};

    // Do not use rookie's `immutable=1` URI for a live Firefox database: the
    // browser may still be updating its WAL. A true read-only SQLite
    // connection participates in the normal WAL locking protocol without
    // writing cookies or asking Firefox to close.
    let connection = Connection::open_with_flags(cookies_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| {
            CliError::Config(format!(
                "failed to open Firefox cookies read-only at {cookies_path}: {error}"
            ))
        })?;
    let mut statement = connection
        .prepare(
            "SELECT host, path, isSecure, expiry, name, value, isHttpOnly, sameSite
             FROM moz_cookies
             WHERE lower(host) = ?1 OR lower(host) LIKE ?2",
        )
        .map_err(|error| CliError::Config(format!("failed to query Firefox cookies: {error}")))?;
    let rows = statement
        .query_map(rusqlite::params!["suno.com", "%.suno.com"], |row| {
            let expires = row.get::<_, i64>(3)?;
            Ok(rookie::enums::Cookie {
                domain: row.get(0)?,
                path: row.get(1)?,
                secure: row.get(2)?,
                expires: (expires > 0).then_some(expires as u64),
                name: row.get(4)?,
                value: row.get(5)?,
                http_only: row.get(6)?,
                same_site: row.get(7).unwrap_or(-1),
            })
        })
        .map_err(|error| CliError::Config(format!("failed to read Firefox cookies: {error}")))?;

    let mut cookies = Vec::new();
    for row in rows {
        let cookie = row.map_err(|error| {
            CliError::Config(format!("failed to decode a Firefox cookie row: {error}"))
        })?;
        if is_suno_cookie_domain(&cookie.domain) {
            cookies.push(cookie);
        }
    }
    Ok(cookies)
}

fn browser_auth_from_cookies_with_environment(
    browser_environment: BrowserEnvironment,
    cookies: Vec<rookie::enums::Cookie>,
    expected_clerk_client_cookie: Option<&str>,
) -> Option<BrowserAuth> {
    let mut seen = HashSet::new();
    let mut header_parts = Vec::new();
    let mut clerk_client_cookie: Option<String> = None;
    let mut auth_domain_clerk: Option<String> = None;
    let mut matching_clerk: Option<String> = None;
    let mut device_id: Option<String> = None;

    for cookie in cookies {
        if !is_suno_cookie_domain(&cookie.domain) {
            continue;
        }
        if cookie.name == "__client" && !cookie.value.is_empty() {
            if expected_clerk_client_cookie == Some(cookie.value.as_str()) {
                matching_clerk = Some(cookie.value.clone());
            }
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

    let clerk_client_cookie = if expected_clerk_client_cookie.is_some() {
        matching_clerk?
    } else {
        auth_domain_clerk.or(clerk_client_cookie)?
    };
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
                client_hints: None,
            },
            vec![
                rookie_cookie("__client", "suno-client", ".suno.com"),
                rookie_cookie("__client", "auth-client", "auth.suno.com"),
                rookie_cookie("ajs_anonymous_id", "%22device-123%22", ".suno.com"),
            ],
            None,
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
                client_hints: None,
            },
            vec![
                rookie_cookie("__client", "auth-client", "auth.suno.com"),
                rookie_cookie("ajs_anonymous_id", "%22device-123%22", ".suno.com"),
            ],
            None,
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

    #[test]
    fn account_recovery_matches_any_client_cookie_in_the_profile() {
        let auth = browser_auth_from_cookies_with_environment(
            BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: None,
                accept_language: None,
                client_hints: None,
            },
            vec![
                rookie_cookie("__client", "site-client", ".suno.com"),
                rookie_cookie("__client", "auth-client", "auth.suno.com"),
            ],
            Some("site-client"),
        )
        .expect("stored cookie should identify the profile");

        assert_eq!(auth.clerk_client_cookie, "site-client");
    }

    #[test]
    fn firefox_cookie_reader_sees_live_wal_updates_without_immutable_mode() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("cookies.sqlite");
        let connection = rusqlite::Connection::open(&path).expect("Firefox test database");
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE moz_cookies (
                    host TEXT, path TEXT, isSecure INTEGER, expiry INTEGER,
                    name TEXT, value TEXT, isHttpOnly INTEGER, sameSite INTEGER
                 );
                 INSERT INTO moz_cookies VALUES (
                    'auth.suno.com', '/', 1, 4102444800,
                    '__client', 'wal-client', 1, 1
                 );
                 INSERT INTO moz_cookies VALUES (
                    'unrelated.example', '/', 1, 'not-an-integer',
                    'private', 'must-not-enter-memory', 1, 1
                 );",
            )
            .expect("write live WAL cookie");

        let cookies = firefox_profile_cookies(path.to_str().expect("Unicode path"))
            .expect("read live Firefox WAL");

        assert_eq!(cookies.len(), 1);
        assert!(cookies.iter().any(|cookie| {
            cookie.name == "__client"
                && cookie.value == "wal-client"
                && cookie.domain == "auth.suno.com"
        }));
    }
}
