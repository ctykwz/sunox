use std::path::{Path, PathBuf};
use std::process::Command;

use super::browser::extract_browser_auth_for_clerk;
use super::interactive::{
    probe_browser_runtime_environment, probe_firefox_runtime_environment,
    recorded_interactive_browser_source,
};
use super::state::AuthState;
use super::types::{BrowserAuth, BrowserEnvironment};
use crate::browser::{installed_chromium_browser_sources, installed_firefox_browser_sources};
use crate::core::CliError;

pub(crate) fn browser_environment_for_profile(
    browser_source: &str,
    profile_dir: &Path,
) -> BrowserEnvironment {
    let accept_language = if browser_source.starts_with("firefox") {
        accept_language_from_firefox_profile(profile_dir)
    } else {
        accept_language_from_chromium_profile(profile_dir)
    };
    BrowserEnvironment {
        browser_source: Some(browser_source.to_string()),
        user_agent: None,
        accept_language,
        client_hints: None,
    }
}

pub(super) fn browser_source_for_firefox_profile(profile_dir: &Path) -> String {
    let raw = match std::fs::read_to_string(profile_dir.join("compatibility.ini")) {
        Ok(raw) => raw,
        Err(_) => return "firefox".into(),
    };
    raw.lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| matches!(key.trim(), "LastPlatformDir" | "LastAppDir"))
        .find_map(|(_, value)| crate::browser::firefox_source_for_path(Path::new(value.trim())))
        .unwrap_or("firefox")
        .to_string()
}

#[cfg(test)]
fn accept_language_from_chromium_user_data_dirs(user_data_dirs: &[PathBuf]) -> Option<String> {
    for user_data_dir in user_data_dirs {
        for preference_path in chromium_preference_paths(user_data_dir) {
            let Ok(raw) = std::fs::read_to_string(preference_path) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                continue;
            };
            let Some(languages) = value
                .get("intl")
                .and_then(|intl| intl.get("accept_languages"))
                .and_then(|languages| languages.as_str())
            else {
                continue;
            };
            if let Some(header) = accept_language_from_preference(languages) {
                return Some(header);
            }
        }
    }
    None
}

fn accept_language_from_chromium_profile(profile_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(profile_dir.join("Preferences")).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let languages = value
        .get("intl")
        .and_then(|intl| intl.get("accept_languages"))
        .and_then(|languages| languages.as_str())?;
    accept_language_from_preference(languages)
}

#[cfg(test)]
fn chromium_preference_paths(user_data_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for profile in ["Default", "Profile 1", "Profile 2", "Profile 3"] {
        let candidate = user_data_dir.join(profile).join("Preferences");
        if candidate.exists() {
            paths.push(candidate);
        }
    }

    if let Ok(entries) = std::fs::read_dir(user_data_dir) {
        for entry in entries.flatten() {
            let candidate = entry.path().join("Preferences");
            if candidate.exists() && !paths.contains(&candidate) {
                paths.push(candidate);
            }
        }
    }
    paths
}

fn accept_language_from_firefox_profile(profile_dir: &Path) -> Option<String> {
    for filename in ["user.js", "prefs.js"] {
        let raw = std::fs::read_to_string(profile_dir.join(filename)).ok();
        if let Some(value) = raw.as_deref().and_then(firefox_accept_languages_pref)
            && let Some(header) = accept_language_from_preference(&value)
        {
            return Some(header);
        }
    }
    None
}

fn firefox_accept_languages_pref(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if !line.starts_with("user_pref(\"intl.accept_languages\"") {
            continue;
        }
        let (_, rest) = line.split_once(',')?;
        let rest = rest.trim();
        let value = rest
            .trim_end_matches(';')
            .trim_end_matches(')')
            .trim()
            .trim_matches('"');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

pub(crate) fn accept_language_from_browser_languages(languages: &[String]) -> Option<String> {
    let mut parts = Vec::new();
    for (index, language) in languages
        .iter()
        .filter_map(|v| non_empty_header_value(Some(v)))
        .enumerate()
    {
        if index == 0 {
            parts.push(language);
        } else {
            let quality = (10_u32.saturating_sub(index as u32)).max(1);
            parts.push(format!("{language};q=0.{quality}"));
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

fn accept_language_from_preference(value: &str) -> Option<String> {
    let languages = value
        .split(',')
        .map(|part| {
            part.split_once(';')
                .map(|(language, _)| language)
                .unwrap_or(part)
                .trim()
                .to_string()
        })
        .filter(|language| !language.is_empty())
        .collect::<Vec<_>>();
    accept_language_from_browser_languages(&languages)
}

pub(crate) fn accept_language_from_system_locale() -> Option<String> {
    let mut locales = Vec::new();
    if let Ok(value) = std::env::var("LANGUAGE") {
        locales.extend(value.split(':').map(str::to_string));
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            locales.push(value);
            break;
        }
    }

    if cfg!(target_os = "macos")
        && let Ok(output) = Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
    {
        locales.splice(0..0, locale_values_from_command_output(&output.stdout));
    } else if cfg!(target_os = "windows")
        && locales.is_empty()
        && let Ok(output) = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "[System.Globalization.CultureInfo]::CurrentCulture.Name",
            ])
            .output()
    {
        locales.extend(locale_values_from_command_output(&output.stdout));
    }

    let languages = locales
        .into_iter()
        .filter_map(|value| normalize_locale_name(&value))
        .collect::<Vec<_>>();
    accept_language_from_browser_languages(&languages)
}

fn locale_values_from_command_output(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| {
            line.trim()
                .trim_matches(['(', ')', ',', '"', ' '])
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn normalize_locale_name(value: &str) -> Option<String> {
    let locale = value.trim();
    let locale = locale
        .split_once('.')
        .map(|(locale, _)| locale)
        .unwrap_or(locale);
    let value = locale
        .split_once('@')
        .map(|(locale, _)| locale)
        .unwrap_or(locale)
        .replace('_', "-");
    if value.is_empty() || matches!(value.as_str(), "C" | "POSIX") {
        None
    } else {
        Some(value)
    }
}

pub(crate) fn non_empty_header_value(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.contains('\r') || value.contains('\n') {
        None
    } else {
        Some(value.to_string())
    }
}

pub(super) fn chromium_user_data_dirs(browser_source: &str) -> Vec<PathBuf> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let home = base_dirs.home_dir();

    if cfg!(target_os = "macos") {
        let app_support = home.join("Library").join("Application Support");
        match browser_source {
            "chrome" => {
                let mut dirs = ["Chrome", "Chrome Beta", "Chrome Dev", "Chrome Canary"]
                    .into_iter()
                    .map(|channel| app_support.join("Google").join(channel))
                    .collect::<Vec<_>>();
                dirs.push(app_support.join("Chromium"));
                dirs
            }
            "edge" => [
                "Microsoft Edge",
                "Microsoft Edge Beta",
                "Microsoft Edge Dev",
                "Microsoft Edge Canary",
            ]
            .into_iter()
            .map(|channel| app_support.join(channel))
            .collect(),
            "brave" => [
                "Brave-Browser",
                "Brave-Browser-Beta",
                "Brave-Browser-Nightly",
            ]
            .into_iter()
            .map(|channel| app_support.join("BraveSoftware").join(channel))
            .collect(),
            "arc" => vec![app_support.join("Arc").join("User Data")],
            _ => Vec::new(),
        }
    } else if cfg!(target_os = "windows") {
        let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
            return Vec::new();
        };
        match browser_source {
            "chrome" => {
                let mut dirs = ["Chrome", "Chrome Beta", "Chrome Dev", "Chrome SxS"]
                    .into_iter()
                    .map(|channel| {
                        local_app_data
                            .join("Google")
                            .join(channel)
                            .join("User Data")
                    })
                    .collect::<Vec<_>>();
                dirs.push(local_app_data.join("Chromium").join("User Data"));
                dirs
            }
            "edge" => ["Edge", "Edge Beta", "Edge Dev", "Edge SxS"]
                .into_iter()
                .map(|channel| {
                    local_app_data
                        .join("Microsoft")
                        .join(channel)
                        .join("User Data")
                })
                .collect(),
            "brave" => [
                "Brave-Browser",
                "Brave-Browser-Beta",
                "Brave-Browser-Nightly",
            ]
            .into_iter()
            .map(|channel| {
                local_app_data
                    .join("BraveSoftware")
                    .join(channel)
                    .join("User Data")
            })
            .collect(),
            _ => Vec::new(),
        }
    } else {
        let config = home.join(".config");
        match browser_source {
            "chrome" => vec![
                config.join("google-chrome"),
                config.join("google-chrome-beta"),
                config.join("google-chrome-unstable"),
                config.join("chromium"),
                home.join("snap")
                    .join("chromium")
                    .join("common")
                    .join("chromium"),
            ],
            "edge" => [
                "microsoft-edge",
                "microsoft-edge-beta",
                "microsoft-edge-dev",
            ]
            .into_iter()
            .map(|channel| config.join(channel))
            .collect(),
            "brave" => [
                "Brave-Browser",
                "Brave-Browser-Beta",
                "Brave-Browser-Nightly",
            ]
            .into_iter()
            .map(|channel| config.join("BraveSoftware").join(channel))
            .collect(),
            _ => Vec::new(),
        }
    }
}

pub(super) fn firefox_profile_dirs() -> Vec<PathBuf> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Vec::new();
    };
    let home = base_dirs.home_dir();

    if cfg!(target_os = "macos") {
        vec![
            home.join("Library")
                .join("Application Support")
                .join("Firefox")
                .join("Profiles"),
        ]
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|app_data| app_data.join("Mozilla").join("Firefox").join("Profiles"))
            .into_iter()
            .collect()
    } else {
        vec![
            home.join(".mozilla").join("firefox"),
            home.join("snap")
                .join("firefox")
                .join("common")
                .join(".mozilla")
                .join("firefox"),
        ]
    }
}

pub(super) fn browser_source_for_profile_root(base_source: &str, user_data_dir: &Path) -> String {
    let normalized = user_data_dir.to_string_lossy().replace('\\', "/");
    let mut components = normalized
        .trim_end_matches('/')
        .rsplit('/')
        .filter(|component| !component.is_empty());
    let leaf = components.next().unwrap_or_default().to_ascii_lowercase();
    let channel = if leaf == "user data" {
        components
            .next()
            .map(str::to_ascii_lowercase)
            .unwrap_or(leaf)
    } else {
        leaf
    };
    match (base_source, channel.as_str()) {
        ("chrome", "chromium") => "chromium",
        ("chrome", "chrome beta" | "google-chrome-beta") => "chrome-beta",
        ("chrome", "chrome dev" | "google-chrome-unstable") => "chrome-dev",
        ("chrome", "chrome canary" | "chrome sxs") => "chrome-canary",
        ("edge", "microsoft edge beta" | "edge beta" | "microsoft-edge-beta") => "edge-beta",
        ("edge", "microsoft edge dev" | "edge dev" | "microsoft-edge-dev") => "edge-dev",
        ("edge", "microsoft edge canary" | "edge sxs") => "edge-canary",
        ("brave", "brave-browser-beta") => "brave-beta",
        ("brave", "brave-browser-nightly") => "brave-nightly",
        _ => base_source,
    }
    .to_string()
}

pub(crate) async fn enrich_browser_auth_environment(
    auth: &mut BrowserAuth,
) -> Result<bool, CliError> {
    if browser_environment_is_complete(auth.browser_environment.as_ref()) {
        return Ok(false);
    }
    let existing = auth.browser_environment.clone();
    let existing_device_id = auth.device_id.clone();
    let (recovered, recovered_device_id) =
        recover_browser_metadata(existing.as_ref(), Some(auth.clerk_client_cookie.as_str()))
            .await?;
    if auth.device_id.is_none() {
        auth.device_id = recovered_device_id;
    }
    if recovered == existing && auth.device_id == existing_device_id {
        Ok(false)
    } else {
        auth.browser_environment = recovered;
        Ok(true)
    }
}

pub(crate) async fn recover_auth_state_environment(auth: &mut AuthState) -> Result<bool, CliError> {
    let device_id_is_present = auth
        .device_id
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    if browser_environment_is_complete(auth.browser_environment.as_ref()) && device_id_is_present {
        return Ok(false);
    }
    let existing = auth.browser_environment.clone();
    let existing_device_id = auth.device_id.clone();
    let (recovered, recovered_device_id) =
        recover_browser_metadata(existing.as_ref(), auth.clerk_client_cookie.as_deref()).await?;
    if !device_id_is_present {
        auth.device_id = recovered_device_id;
    }
    if recovered == existing && auth.device_id == existing_device_id {
        Ok(false)
    } else {
        auth.browser_environment = recovered;
        Ok(true)
    }
}

/// Load persisted auth and repair legacy browser metadata, including the
/// matching device identity, before it can be copied into a refreshed state.
/// The compare-and-swap save prevents a local probe from overwriting a
/// concurrent login, logout, or account switch.
pub(crate) async fn load_auth_state_with_recovered_environment() -> Result<AuthState, CliError> {
    let original = AuthState::load()?;
    let mut auth = original.clone();
    if recover_auth_state_environment(&mut auth).await? {
        auth.save_if_unchanged(Some(&original))?;
    }
    Ok(auth)
}

fn browser_environment_is_complete(environment: Option<&BrowserEnvironment>) -> bool {
    environment.is_some_and(|environment| {
        let user_agent = non_empty_header_value(environment.user_agent.as_deref());
        let chromium = user_agent
            .as_deref()
            .is_some_and(|value| value.contains("Chrome/") || value.contains("Edg/"));
        let client_hints_complete = !chromium
            || environment.client_hints.as_ref().is_some_and(|hints| {
                non_empty_header_value(Some(&hints.sec_ch_ua)).is_some()
                    && non_empty_header_value(Some(&hints.sec_ch_ua_mobile)).is_some()
                    && non_empty_header_value(Some(&hints.sec_ch_ua_platform)).is_some()
            });
        non_empty_header_value(environment.browser_source.as_deref()).is_some()
            && user_agent.is_some()
            && non_empty_header_value(environment.accept_language.as_deref()).is_some()
            && client_hints_complete
    })
}

async fn recover_browser_metadata(
    existing: Option<&BrowserEnvironment>,
    expected_clerk_client_cookie: Option<&str>,
) -> Result<(Option<BrowserEnvironment>, Option<String>), CliError> {
    let matching_browser = match expected_clerk_client_cookie {
        Some(cookie) => {
            let cookie = cookie.to_string();
            tokio::task::spawn_blocking(move || extract_browser_auth_for_clerk(&cookie))
                .await
                .ok()
                .and_then(Result::ok)
        }
        None => None,
    };
    let recovered_device_id = matching_browser
        .as_ref()
        .and_then(|auth| auth.device_id.clone());
    let matching_browser_environment = matching_browser.and_then(|auth| auth.browser_environment);

    let Some(source) = matching_browser_environment
        .as_ref()
        .and_then(|environment| environment.browser_source.clone())
        .or_else(|| {
            existing.and_then(|environment| {
                non_empty_header_value(environment.browser_source.as_deref())
            })
        })
        .or_else(preferred_installed_browser_source)
    else {
        return Ok((existing.cloned(), recovered_device_id));
    };
    let source = if source == "interactive-browser" {
        let Some(source) =
            recorded_interactive_browser_source().or_else(preferred_installed_browser_source)
        else {
            return Ok((existing.cloned(), recovered_device_id));
        };
        source
    } else {
        source
    };

    let existing = existing.cloned().map(|mut environment| {
        if environment.browser_source.as_deref() == Some("interactive-browser") {
            environment.browser_source = Some(source.clone());
        }
        environment
    });

    let profile_environment = matching_browser_environment.or_else(|| {
        Some(BrowserEnvironment {
            browser_source: Some(source.clone()),
            user_agent: None,
            accept_language: None,
            client_hints: None,
        })
    });
    let runtime_environment = if source.starts_with("firefox") {
        if installed_firefox_browser_sources()
            .iter()
            .any(|item| item == &source)
        {
            match probe_firefox_runtime_environment(&source).await {
                Ok(environment) => Some(environment),
                Err(error) => {
                    eprintln!(
                        "Warning: could not refresh Firefox request metadata ({error}); preserving stored values when available"
                    );
                    None
                }
            }
        } else {
            None
        }
    } else {
        let installed = crate::browser::locate_chromium_browser_for_source(&source).is_ok();
        if installed {
            match probe_browser_runtime_environment(&source).await {
                Ok(environment) => {
                    if !browser_environment_is_complete(Some(&environment)) {
                        eprintln!(
                            "Warning: {source} returned partial request metadata; preserving stored values for missing fields"
                        );
                    }
                    Some(environment)
                }
                Err(error) => {
                    eprintln!(
                        "Warning: could not refresh {source} request metadata ({error}); preserving stored values when available"
                    );
                    None
                }
            }
        } else {
            None
        }
    };

    Ok((
        merge_recovered_browser_environment(profile_environment, runtime_environment, existing),
        recovered_device_id,
    ))
}

fn merge_recovered_browser_environment(
    profile_environment: Option<BrowserEnvironment>,
    runtime_environment: Option<BrowserEnvironment>,
    existing: Option<BrowserEnvironment>,
) -> Option<BrowserEnvironment> {
    let recovered = merge_fresh_browser_environments(profile_environment, runtime_environment);
    merge_browser_environments(recovered, existing)
}

fn merge_fresh_browser_environments(
    profile: Option<BrowserEnvironment>,
    runtime: Option<BrowserEnvironment>,
) -> Option<BrowserEnvironment> {
    match (profile, runtime) {
        (Some(profile), Some(runtime)) => Some(BrowserEnvironment {
            browser_source: profile.browser_source.or(runtime.browser_source),
            user_agent: runtime.user_agent.or(profile.user_agent),
            accept_language: profile.accept_language.or(runtime.accept_language),
            client_hints: runtime.client_hints.or(profile.client_hints),
        }),
        (Some(environment), None) | (None, Some(environment)) => Some(environment),
        (None, None) => None,
    }
}

fn preferred_installed_browser_source() -> Option<String> {
    let mut sources = installed_chromium_browser_sources();
    sources.extend(installed_firefox_browser_sources());
    (sources.len() == 1).then(|| sources.remove(0))
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
            client_hints: primary.client_hints.or(fallback.client_hints),
        }),
        (Some(environment), None) | (None, Some(environment)) => Some(environment),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn write_file(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(path, content).expect("write file");
    }

    #[test]
    fn chromium_preferences_become_accept_language_header() {
        let root =
            std::env::temp_dir().join(format!("sunox-browser-env-test-{}", uuid::Uuid::new_v4()));
        write_file(
            &root.join("Default").join("Preferences"),
            r#"{"intl":{"accept_languages":"zh-CN,zh,en-US,en"}}"#,
        );

        let header = accept_language_from_chromium_user_data_dirs(std::slice::from_ref(&root))
            .expect("accept language");

        assert_eq!(header, "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn chromium_preferences_scan_profiles_until_language_is_found() {
        let root =
            std::env::temp_dir().join(format!("sunox-browser-env-test-{}", uuid::Uuid::new_v4()));
        write_file(&root.join("Default").join("Preferences"), r#"{"intl":{}}"#);
        write_file(
            &root.join("Profile 1").join("Preferences"),
            r#"{"intl":{"accept_languages":"ja,en-US,en"}}"#,
        );

        let header = accept_language_from_chromium_user_data_dirs(std::slice::from_ref(&root))
            .expect("accept language");

        assert_eq!(header, "ja,en-US;q=0.9,en;q=0.8");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_runtime_probe_keeps_stored_ua_but_uses_new_profile_fields() {
        let merged = merge_recovered_browser_environment(
            Some(BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: None,
                accept_language: Some("zh-CN,zh;q=0.9".into()),
                client_hints: None,
            }),
            None,
            Some(BrowserEnvironment {
                browser_source: Some("edge".into()),
                user_agent: Some("Mozilla/5.0 Stored Edg/149.0.0.0".into()),
                accept_language: Some("en-US,en;q=0.9".into()),
                client_hints: None,
            }),
        )
        .expect("merged environment");

        assert_eq!(merged.browser_source.as_deref(), Some("chrome"));
        assert_eq!(
            merged.user_agent.as_deref(),
            Some("Mozilla/5.0 Stored Edg/149.0.0.0")
        );
        assert_eq!(merged.accept_language.as_deref(), Some("zh-CN,zh;q=0.9"));
    }

    #[test]
    fn fresh_runtime_ua_replaces_stored_ua() {
        let merged = merge_recovered_browser_environment(
            Some(BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: None,
                accept_language: Some("zh-CN,zh;q=0.9".into()),
                client_hints: None,
            }),
            Some(BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: Some("Mozilla/5.0 Chrome/150.0.0.0".into()),
                accept_language: Some("en-US,en;q=0.9".into()),
                client_hints: None,
            }),
            Some(BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: Some("Mozilla/5.0 Chrome/149.0.0.0".into()),
                accept_language: Some("ja,en;q=0.9".into()),
                client_hints: Some(super::super::types::BrowserClientHints {
                    sec_ch_ua: r#""Chromium";v="149""#.into(),
                    sec_ch_ua_mobile: "?0".into(),
                    sec_ch_ua_platform: r#""macOS""#.into(),
                }),
            }),
        )
        .expect("merged environment");

        assert_eq!(
            merged.user_agent.as_deref(),
            Some("Mozilla/5.0 Chrome/150.0.0.0")
        );
        assert_eq!(merged.accept_language.as_deref(), Some("zh-CN,zh;q=0.9"));
        assert_eq!(
            merged.client_hints,
            Some(super::super::types::BrowserClientHints {
                sec_ch_ua: r#""Chromium";v="149""#.into(),
                sec_ch_ua_mobile: "?0".into(),
                sec_ch_ua_platform: r#""macOS""#.into(),
            })
        );
    }

    #[test]
    fn profile_root_preserves_preview_channel_identity() {
        assert_eq!(
            browser_source_for_profile_root(
                "chrome",
                Path::new("/Applications/Google/Chrome Canary")
            ),
            "chrome-canary"
        );
        assert_eq!(
            browser_source_for_profile_root(
                "edge",
                Path::new(r"C:\Users\me\AppData\Local\Microsoft\Edge Beta\User Data")
            ),
            "edge-beta"
        );
        assert_eq!(
            browser_source_for_profile_root(
                "brave",
                Path::new("/home/me/.config/BraveSoftware/Brave-Browser-Nightly")
            ),
            "brave-nightly"
        );
        assert_eq!(
            browser_source_for_profile_root("chrome", Path::new("/home/me/.config/chromium")),
            "chromium"
        );
        assert_eq!(
            browser_source_for_profile_root(
                "chrome",
                Path::new("/Users/dev/Library/Application Support/Google/Chrome")
            ),
            "chrome"
        );
        assert_eq!(
            browser_source_for_profile_root(
                "chrome",
                Path::new("/home/betauser/.config/google-chrome")
            ),
            "chrome"
        );
    }

    #[test]
    fn system_locale_values_become_browser_language_headers() {
        let languages = ["zh_CN.UTF-8", "en_US@calendar=gregorian"]
            .into_iter()
            .filter_map(normalize_locale_name)
            .collect::<Vec<_>>();

        assert_eq!(languages, ["zh-CN", "en-US"]);
        assert_eq!(
            accept_language_from_browser_languages(&languages).as_deref(),
            Some("zh-CN,en-US;q=0.9")
        );
        assert_eq!(normalize_locale_name("C"), None);
    }

    #[test]
    fn firefox_profile_uses_its_recorded_install_channel() {
        let root =
            std::env::temp_dir().join(format!("sunox-firefox-env-test-{}", uuid::Uuid::new_v4()));
        write_file(
            &root.join("compatibility.ini"),
            "[Compatibility]\nLastPlatformDir=/Applications/Firefox Developer Edition.app/Contents/Resources\n",
        );

        assert_eq!(
            browser_source_for_firefox_profile(&root),
            "firefox-developer"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn complete_interactive_environment_is_not_reprobed() {
        let mut auth = BrowserAuth {
            clerk_client_cookie: "unused-cookie".into(),
            cookie_header: "__client=unused-cookie".into(),
            device_id: None,
            browser_environment: Some(BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: Some("Mozilla/5.0 Chrome/150.0.0.0".into()),
                accept_language: Some("zh-CN,en;q=0.9".into()),
                client_hints: Some(super::super::types::BrowserClientHints {
                    sec_ch_ua: r#""Chromium";v="150""#.into(),
                    sec_ch_ua_mobile: "?0".into(),
                    sec_ch_ua_platform: r#""macOS""#.into(),
                }),
            }),
        };
        let before = auth.browser_environment.clone();

        assert!(
            !enrich_browser_auth_environment(&mut auth)
                .await
                .expect("enrich")
        );
        assert_eq!(auth.browser_environment, before);
    }
}
