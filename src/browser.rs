use std::path::{Path, PathBuf};

use crate::core::CliError;

const BROWSER_PATH_ENV: &str = "SUNOX_BROWSER_PATH";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TargetOs {
    Macos,
    Linux,
    Windows,
}

pub fn locate_chromium_browser() -> Result<PathBuf, CliError> {
    if let Some(path) = configured_browser_path(|key| std::env::var_os(key))? {
        if firefox_source_for_path(&path).is_some() {
            return Err(CliError::Config(format!(
                "{BROWSER_PATH_ENV} points to Firefox, but interactive login requires a Chromium-family browser"
            )));
        }
        return Ok(path);
    }

    for candidate in chromium_browser_candidates(current_target_os(), |key| {
        std::env::var_os(key).map(PathBuf::from)
    }) {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(CliError::Config(
        "Could not find a Chrome, Edge, or Chromium binary. Install a Chromium-based browser or set SUNOX_BROWSER_PATH."
            .into(),
    ))
}

/// Locate the installed Chromium-family browser that produced an extracted
/// cookie set. Keeping this source-specific prevents Chrome cookies from being
/// paired with Edge or Brave runtime headers merely because that binary appears
/// first in the generic browser search order.
pub(crate) fn locate_chromium_browser_for_source(
    browser_source: &str,
) -> Result<PathBuf, CliError> {
    if let Some(path) = configured_browser_path(|key| std::env::var_os(key))? {
        let source = normalize_browser_source(browser_source);
        let detected_source = browser_source_for_path(&path);
        if detected_source.is_none() || detected_source == Some(source) {
            return Ok(path);
        }
        return Err(CliError::Config(format!(
            "{BROWSER_PATH_ENV} points to {}, which does not match browser source {source}",
            path.display()
        )));
    }

    let source = normalize_browser_source(browser_source);
    for candidate in chromium_browser_candidates(current_target_os(), |key| {
        std::env::var_os(key).map(PathBuf::from)
    }) {
        if browser_path_matches_source(&candidate, source) && candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(CliError::Config(format!(
        "Could not find the installed {source} browser binary needed to recover browser request metadata"
    )))
}

pub(crate) fn installed_chromium_browser_sources() -> Vec<String> {
    if let Ok(Some(path)) = configured_browser_path(|key| std::env::var_os(key)) {
        if let Some(source) = browser_source_for_path(&path) {
            return vec![source.to_string()];
        }
        if firefox_source_for_path(&path).is_some() {
            return Vec::new();
        }
        return vec!["chrome".to_string()];
    }

    let candidates = chromium_browser_candidates(current_target_os(), |key| {
        std::env::var_os(key).map(PathBuf::from)
    });
    let mut sources = Vec::new();
    for path in candidates.into_iter().filter(|path| path.exists()) {
        if let Some(source) = browser_source_for_path(&path)
            && !sources.iter().any(|item| item == source)
        {
            sources.push(source.to_string());
        }
    }
    sources
}

pub(crate) fn locate_firefox_browser_for_source(source: &str) -> Result<PathBuf, CliError> {
    if let Some(path) = configured_browser_path(|key| std::env::var_os(key))? {
        if firefox_source_for_path(&path) == Some(source) {
            return Ok(path);
        }
        return Err(CliError::Config(format!(
            "{BROWSER_PATH_ENV} points to {}, which does not match browser source {source}",
            path.display()
        )));
    }

    firefox_browser_candidates(current_target_os())
        .into_iter()
        .find_map(|(candidate_source, path)| {
            (candidate_source == source && path.is_file()).then_some(path)
        })
        .ok_or_else(|| {
            CliError::Config(format!(
                "Could not find the installed {source} browser binary needed to recover browser request metadata"
            ))
        })
}

pub(crate) fn installed_firefox_browser_sources() -> Vec<String> {
    if let Ok(Some(path)) = configured_browser_path(|key| std::env::var_os(key)) {
        if let Some(source) = firefox_source_for_path(&path) {
            return vec![source.to_string()];
        }
        return Vec::new();
    }

    let mut sources = Vec::new();
    for (source, path) in firefox_browser_candidates(current_target_os()) {
        if path.is_file() && !sources.iter().any(|item| item == source) {
            sources.push(source.to_string());
        }
    }
    sources
}

pub(crate) fn firefox_source_for_path(path: &Path) -> Option<&'static str> {
    let identity = browser_binary_identity(path);
    match (
        identity.executable.as_str(),
        identity.install_dir.as_str(),
        identity.app_bundle.as_str(),
    ) {
        ("firefox-developer-edition", _, _)
        | ("firefoxdeveloperedition", _, _)
        | (_, "firefox developer edition", _)
        | (_, _, "firefox developer edition.app") => Some("firefox-developer"),
        ("firefox-nightly", _, _) | (_, "firefox nightly", _) | (_, _, "firefox nightly.app") => {
            Some("firefox-nightly")
        }
        ("firefox-beta", _, _) | (_, "mozilla firefox beta", _) | (_, _, "firefox beta.app") => {
            Some("firefox-beta")
        }
        ("firefox" | "firefox.exe", _, _) | (_, "mozilla firefox", _) | (_, _, "firefox.app") => {
            Some("firefox")
        }
        _ => None,
    }
}

fn firefox_browser_candidates(target: TargetOs) -> Vec<(&'static str, PathBuf)> {
    match target {
        TargetOs::Macos => vec![
            (
                "firefox",
                PathBuf::from("/Applications/Firefox.app/Contents/MacOS/firefox"),
            ),
            (
                "firefox-beta",
                PathBuf::from("/Applications/Firefox Beta.app/Contents/MacOS/firefox"),
            ),
            (
                "firefox-developer",
                PathBuf::from("/Applications/Firefox Developer Edition.app/Contents/MacOS/firefox"),
            ),
            (
                "firefox-nightly",
                PathBuf::from("/Applications/Firefox Nightly.app/Contents/MacOS/firefox"),
            ),
        ],
        TargetOs::Windows => [
            ("firefox", "Mozilla Firefox"),
            ("firefox-beta", "Mozilla Firefox Beta"),
            ("firefox-developer", "Firefox Developer Edition"),
            ("firefox-nightly", "Firefox Nightly"),
        ]
        .into_iter()
        .flat_map(|(source, directory)| {
            [r"C:\Program Files", r"C:\Program Files (x86)"]
                .into_iter()
                .map(move |root| {
                    (
                        source,
                        PathBuf::from(root).join(directory).join("firefox.exe"),
                    )
                })
        })
        .collect(),
        TargetOs::Linux => vec![
            ("firefox", PathBuf::from("/usr/bin/firefox")),
            ("firefox", PathBuf::from("/usr/local/bin/firefox")),
            ("firefox-beta", PathBuf::from("/usr/bin/firefox-beta")),
            (
                "firefox-developer",
                PathBuf::from("/usr/bin/firefox-developer-edition"),
            ),
            ("firefox-nightly", PathBuf::from("/usr/bin/firefox-nightly")),
        ],
    }
}

fn normalize_browser_source(source: &str) -> &str {
    match source {
        "interactive-browser" => "chrome",
        other => other,
    }
}

fn browser_path_matches_source(path: &Path, source: &str) -> bool {
    detect_chromium_browser_source(path) == Some(source)
}

pub(crate) fn browser_source_for_path(path: &Path) -> Option<&'static str> {
    detect_chromium_browser_source(path)
}

struct BrowserBinaryIdentity {
    executable: String,
    install_dir: String,
    app_bundle: String,
}

fn browser_binary_identity(path: &Path) -> BrowserBinaryIdentity {
    let normalized = path.to_string_lossy().replace('\\', "/");
    let components = normalized
        .split('/')
        .map(str::trim)
        .filter(|component| !component.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let executable = components.last().cloned().unwrap_or_default();
    let install_dir = if components
        .get(components.len().saturating_sub(2))
        .is_some_and(|component| component == "application")
    {
        components
            .get(components.len().saturating_sub(3))
            .cloned()
            .unwrap_or_default()
    } else {
        components
            .get(components.len().saturating_sub(2))
            .cloned()
            .unwrap_or_default()
    };
    let app_bundle = components
        .iter()
        .rev()
        .take(5)
        .find(|component| component.ends_with(".app"))
        .cloned()
        .unwrap_or_default();
    BrowserBinaryIdentity {
        executable,
        install_dir,
        app_bundle,
    }
}

fn detect_chromium_browser_source(path: &Path) -> Option<&'static str> {
    let identity = browser_binary_identity(path);
    match (
        identity.executable.as_str(),
        identity.install_dir.as_str(),
        identity.app_bundle.as_str(),
    ) {
        ("google-chrome-beta" | "google chrome beta", _, _)
        | (_, "chrome beta", _)
        | (_, _, "google chrome beta.app") => Some("chrome-beta"),
        ("google-chrome-unstable" | "google chrome dev", _, _)
        | (_, "chrome dev", _)
        | (_, _, "google chrome dev.app") => Some("chrome-dev"),
        ("google chrome canary", _, _)
        | (_, "chrome sxs", _)
        | (_, _, "google chrome canary.app") => Some("chrome-canary"),
        ("chromium" | "chromium-browser", _, _) | (_, "chromium", _) | (_, _, "chromium.app") => {
            Some("chromium")
        }
        ("microsoft-edge-beta" | "microsoft edge beta", _, _)
        | (_, "edge beta", _)
        | (_, _, "microsoft edge beta.app") => Some("edge-beta"),
        ("microsoft-edge-dev" | "microsoft edge dev", _, _)
        | (_, "edge dev", _)
        | (_, _, "microsoft edge dev.app") => Some("edge-dev"),
        ("microsoft edge canary", _, _)
        | (_, "edge sxs", _)
        | (_, _, "microsoft edge canary.app") => Some("edge-canary"),
        ("brave-browser-beta" | "brave browser beta", _, _)
        | (_, "brave-browser-beta", _)
        | (_, _, "brave browser beta.app") => Some("brave-beta"),
        ("brave-browser-nightly" | "brave browser nightly", _, _)
        | (_, "brave-browser-nightly", _)
        | (_, _, "brave browser nightly.app") => Some("brave-nightly"),
        ("google-chrome" | "google-chrome-stable" | "google chrome", _, _)
        | (_, "chrome", _)
        | (_, _, "google chrome.app") => Some("chrome"),
        ("microsoft-edge" | "microsoft-edge-stable" | "microsoft edge" | "msedge.exe", _, _)
        | (_, "edge", _)
        | (_, _, "microsoft edge.app") => Some("edge"),
        ("brave-browser" | "brave-browser-stable" | "brave browser" | "brave.exe", _, _)
        | (_, "brave-browser", _)
        | (_, _, "brave browser.app") => Some("brave"),
        ("arc" | "arc.exe", _, _) | (_, _, "arc.app") => Some("arc"),
        _ => None,
    }
}

fn configured_browser_path<F>(env_var: F) -> Result<Option<PathBuf>, CliError>
where
    F: Fn(&str) -> Option<std::ffi::OsString>,
{
    let Some(path) = env_var(BROWSER_PATH_ENV) else {
        return Ok(None);
    };
    let path = path
        .to_str()
        .map(str::trim)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(path));
    if path.as_os_str().is_empty() {
        return Ok(None);
    }
    if path.exists() {
        return Ok(Some(path));
    }

    Err(CliError::Config(format!(
        "{BROWSER_PATH_ENV} points to a missing file: {}",
        path.display()
    )))
}

fn current_target_os() -> TargetOs {
    if cfg!(target_os = "macos") {
        TargetOs::Macos
    } else if cfg!(target_os = "linux") {
        TargetOs::Linux
    } else {
        TargetOs::Windows
    }
}

fn chromium_browser_candidates<F>(target_os: TargetOs, env_var: F) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<PathBuf>,
{
    match target_os {
        TargetOs::Macos => [
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Beta.app/Contents/MacOS/Google Chrome Beta",
            "/Applications/Google Chrome Dev.app/Contents/MacOS/Google Chrome Dev",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Microsoft Edge Beta.app/Contents/MacOS/Microsoft Edge Beta",
            "/Applications/Microsoft Edge Dev.app/Contents/MacOS/Microsoft Edge Dev",
            "/Applications/Microsoft Edge Canary.app/Contents/MacOS/Microsoft Edge Canary",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            "/Applications/Brave Browser Beta.app/Contents/MacOS/Brave Browser Beta",
            "/Applications/Brave Browser Nightly.app/Contents/MacOS/Brave Browser Nightly",
            "/Applications/Arc.app/Contents/MacOS/Arc",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        TargetOs::Linux => [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/google-chrome-beta",
            "/usr/bin/google-chrome-unstable",
            "/usr/bin/microsoft-edge",
            "/usr/bin/microsoft-edge-stable",
            "/usr/bin/microsoft-edge-beta",
            "/usr/bin/microsoft-edge-dev",
            "/usr/bin/brave-browser",
            "/usr/bin/brave-browser-stable",
            "/usr/bin/brave-browser-beta",
            "/usr/bin/brave-browser-nightly",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/snap/bin/chromium",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        TargetOs::Windows => {
            let mut candidates = Vec::new();
            if let Some(local_app_data) = env_var("LOCALAPPDATA") {
                candidates.extend([
                    append_windows_path(&local_app_data, r"Google\Chrome\Application\chrome.exe"),
                    append_windows_path(
                        &local_app_data,
                        r"Google\Chrome Beta\Application\chrome.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"Google\Chrome Dev\Application\chrome.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"Google\Chrome SxS\Application\chrome.exe",
                    ),
                    append_windows_path(&local_app_data, r"Microsoft\Edge\Application\msedge.exe"),
                    append_windows_path(
                        &local_app_data,
                        r"Microsoft\Edge Beta\Application\msedge.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"Microsoft\Edge Dev\Application\msedge.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"Microsoft\Edge SxS\Application\msedge.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"BraveSoftware\Brave-Browser\Application\brave.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"BraveSoftware\Brave-Browser-Beta\Application\brave.exe",
                    ),
                    append_windows_path(
                        &local_app_data,
                        r"BraveSoftware\Brave-Browser-Nightly\Application\brave.exe",
                    ),
                    append_windows_path(&local_app_data, r"Chromium\Application\chrome.exe"),
                    append_windows_path(&local_app_data, r"Microsoft\WindowsApps\Arc.exe"),
                    append_windows_path(
                        &local_app_data,
                        r"Microsoft\WindowsApps\TheBrowserCompany.Arc_ttt1ap7aakyb4\Arc.exe",
                    ),
                ]);
            }
            candidates.extend([
                PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
                PathBuf::from(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
                PathBuf::from(r"C:\Program Files\Microsoft\Edge Beta\Application\msedge.exe"),
                PathBuf::from(r"C:\Program Files\Microsoft\Edge Dev\Application\msedge.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge Beta\Application\msedge.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge Dev\Application\msedge.exe"),
                PathBuf::from(
                    r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
                ),
                PathBuf::from(
                    r"C:\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe",
                ),
                PathBuf::from(
                    r"C:\Program Files\BraveSoftware\Brave-Browser-Beta\Application\brave.exe",
                ),
                PathBuf::from(
                    r"C:\Program Files\BraveSoftware\Brave-Browser-Nightly\Application\brave.exe",
                ),
            ]);
            candidates
        }
    }
}

fn append_windows_path(base: &Path, relative: &str) -> PathBuf {
    let base = base.to_string_lossy();
    let base = base.trim_end_matches(['\\', '/']);
    PathBuf::from(format!(r"{base}\{relative}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_path_env_uses_configured_browser_path() {
        let browser_path =
            std::env::temp_dir().join(format!("sunox-browser-path-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&browser_path, "").expect("browser stub");

        let configured = configured_browser_path(|key| match key {
            "SUNOX_BROWSER_PATH" => Some(browser_path.clone().into_os_string()),
            _ => None,
        })
        .expect("configured path")
        .expect("path");

        assert_eq!(configured, browser_path);
        let _ = std::fs::remove_file(browser_path);
    }

    #[test]
    fn old_suno_browser_path_env_is_ignored() {
        let browser_path =
            std::env::temp_dir().join(format!("sunox-browser-path-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&browser_path, "").expect("browser stub");

        let configured = configured_browser_path(|key| match key {
            "SUNO_BROWSER_PATH" => Some(browser_path.clone().into_os_string()),
            _ => None,
        })
        .expect("configured path");

        assert!(configured.is_none());
        let _ = std::fs::remove_file(browser_path);
    }

    #[test]
    fn browser_path_env_trims_accidental_whitespace() {
        let browser_path =
            std::env::temp_dir().join(format!("sunox-browser-path-test-{}", uuid::Uuid::new_v4()));
        std::fs::write(&browser_path, "").expect("browser stub");
        let configured = configured_browser_path(|key| match key {
            "SUNOX_BROWSER_PATH" => Some(format!("  {}  ", browser_path.display()).into()),
            _ => None,
        })
        .expect("configured path")
        .expect("path");

        assert_eq!(configured, browser_path);
        let _ = std::fs::remove_file(browser_path);
    }

    #[test]
    fn windows_chrome_candidates_include_per_user_install_path() {
        let candidates = chromium_browser_candidates(TargetOs::Windows, |key| match key {
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\alice\AppData\Local")),
            _ => None,
        });

        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\Google\Chrome\Application\chrome.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe"
        )));
    }

    #[test]
    fn windows_browser_candidates_include_edge_install_paths() {
        let candidates = chromium_browser_candidates(TargetOs::Windows, |key| match key {
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\alice\AppData\Local")),
            _ => None,
        });

        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\Microsoft\Edge\Application\msedge.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\Microsoft\Edge Beta\Application\msedge.exe"
        )));
    }

    #[test]
    fn windows_browser_candidates_include_brave_chromium_and_preview_channels() {
        let candidates = chromium_browser_candidates(TargetOs::Windows, |key| match key {
            "LOCALAPPDATA" => Some(PathBuf::from(r"C:\Users\alice\AppData\Local")),
            _ => None,
        });

        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\BraveSoftware\Brave-Browser\Application\brave.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\Chromium\Application\chrome.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\Google\Chrome Dev\Application\chrome.exe"
        )));
        assert!(candidates.contains(&PathBuf::from(
            r"C:\Users\alice\AppData\Local\BraveSoftware\Brave-Browser-Nightly\Application\brave.exe"
        )));
    }

    #[test]
    fn macos_and_linux_candidates_include_edge() {
        let macos = chromium_browser_candidates(TargetOs::Macos, |_| None);
        let linux = chromium_browser_candidates(TargetOs::Linux, |_| None);

        assert!(macos.contains(&PathBuf::from(
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"
        )));
        assert!(linux.contains(&PathBuf::from("/usr/bin/microsoft-edge")));
        assert!(linux.contains(&PathBuf::from("/usr/bin/microsoft-edge-stable")));
        assert!(linux.contains(&PathBuf::from("/usr/bin/brave-browser")));
        assert!(macos.contains(&PathBuf::from(
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary"
        )));
        assert!(macos.contains(&PathBuf::from("/Applications/Arc.app/Contents/MacOS/Arc")));
    }

    #[test]
    fn source_specific_browser_matching_does_not_cross_browser_families() {
        let chrome = Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        let edge = Path::new("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge");
        let brave = Path::new("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser");

        assert!(browser_path_matches_source(chrome, "chrome"));
        assert!(!browser_path_matches_source(edge, "chrome"));
        assert!(browser_path_matches_source(edge, "edge"));
        assert!(browser_path_matches_source(brave, "brave"));
        assert!(!browser_path_matches_source(brave, "edge"));
        assert_eq!(browser_source_for_path(chrome), Some("chrome"));
        assert_eq!(browser_source_for_path(edge), Some("edge"));
        assert_eq!(
            browser_source_for_path(Path::new(
                "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary"
            )),
            Some("chrome-canary")
        );
        assert!(browser_path_matches_source(
            Path::new("/usr/bin/google-chrome-beta"),
            "chrome-beta"
        ));
        assert!(!browser_path_matches_source(
            Path::new("/usr/bin/google-chrome-beta"),
            "chrome"
        ));
        assert_eq!(
            browser_source_for_path(Path::new("/usr/bin/google-chrome-beta")),
            Some("chrome-beta")
        );
        assert_eq!(
            browser_source_for_path(Path::new(
                r"C:\Users\chromium\AppData\Local\Google\Chrome\Application\chrome.exe"
            )),
            Some("chrome")
        );
    }

    #[test]
    fn firefox_install_paths_preserve_release_channel() {
        assert_eq!(
            firefox_source_for_path(Path::new(
                "/Applications/Firefox.app/Contents/MacOS/firefox"
            )),
            Some("firefox")
        );
        assert_eq!(
            firefox_source_for_path(Path::new(
                "/Applications/Firefox Developer Edition.app/Contents/Resources"
            )),
            Some("firefox-developer")
        );
        assert_eq!(
            firefox_source_for_path(Path::new(r"C:\Program Files\Firefox Nightly\firefox.exe")),
            Some("firefox-nightly")
        );
        assert_eq!(
            firefox_source_for_path(Path::new("/Users/firefox-user/bin/google-chrome")),
            None
        );
        assert_eq!(
            firefox_source_for_path(Path::new(
                r"C:\Users\firefox\AppData\Local\Google\Chrome\Application\chrome.exe"
            )),
            None
        );
    }
}
