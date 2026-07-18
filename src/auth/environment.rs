use std::path::{Path, PathBuf};

use super::types::BrowserEnvironment;

pub(crate) fn browser_environment_for_cookie_source(browser_source: &str) -> BrowserEnvironment {
    BrowserEnvironment {
        browser_source: Some(browser_source.to_string()),
        user_agent: None,
        accept_language: accept_language_from_local_browser(browser_source),
    }
}

fn accept_language_from_local_browser(browser_source: &str) -> Option<String> {
    if browser_source == "firefox" {
        return accept_language_from_firefox_profile_dirs(&firefox_profile_dirs());
    }
    accept_language_from_chromium_user_data_dirs(&chromium_user_data_dirs(browser_source))
}

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

fn accept_language_from_firefox_profile_dirs(profile_roots: &[PathBuf]) -> Option<String> {
    for profile_root in profile_roots {
        let Ok(entries) = std::fs::read_dir(profile_root) else {
            continue;
        };
        for entry in entries.flatten() {
            let profile_dir = entry.path();
            for filename in ["user.js", "prefs.js"] {
                let prefs_path = profile_dir.join(filename);
                let Ok(raw) = std::fs::read_to_string(prefs_path) else {
                    continue;
                };
                if let Some(value) = firefox_accept_languages_pref(&raw)
                    && let Some(header) = accept_language_from_preference(&value)
                {
                    return Some(header);
                }
            }
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
            "chrome" => vec![app_support.join("Google").join("Chrome")],
            "edge" => vec![app_support.join("Microsoft Edge")],
            "brave" => vec![app_support.join("BraveSoftware").join("Brave-Browser")],
            "arc" => vec![app_support.join("Arc").join("User Data")],
            _ => Vec::new(),
        }
    } else if cfg!(target_os = "windows") {
        let Some(local_app_data) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
            return Vec::new();
        };
        match browser_source {
            "chrome" => ["Chrome", "Chrome Beta", "Chrome Dev", "Chrome SxS"]
                .into_iter()
                .map(|channel| {
                    local_app_data
                        .join("Google")
                        .join(channel)
                        .join("User Data")
                })
                .collect(),
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
            "chrome" => vec![config.join("google-chrome"), config.join("chromium")],
            "edge" => vec![config.join("microsoft-edge")],
            "brave" => vec![config.join("BraveSoftware").join("Brave-Browser")],
            _ => Vec::new(),
        }
    }
}

fn firefox_profile_dirs() -> Vec<PathBuf> {
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
        vec![home.join(".mozilla").join("firefox")]
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
}
