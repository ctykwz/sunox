use std::path::PathBuf;

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
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        TargetOs::Linux => [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/microsoft-edge",
            "/usr/bin/microsoft-edge-stable",
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
                    local_app_data.join(r"Google\Chrome\Application\chrome.exe"),
                    local_app_data.join(r"Google\Chrome Beta\Application\chrome.exe"),
                    local_app_data.join(r"Google\Chrome Dev\Application\chrome.exe"),
                    local_app_data.join(r"Google\Chrome SxS\Application\chrome.exe"),
                    local_app_data.join(r"Microsoft\Edge\Application\msedge.exe"),
                    local_app_data.join(r"Microsoft\Edge Beta\Application\msedge.exe"),
                    local_app_data.join(r"Microsoft\Edge Dev\Application\msedge.exe"),
                    local_app_data.join(r"Microsoft\Edge SxS\Application\msedge.exe"),
                    local_app_data.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"),
                    local_app_data.join(r"Chromium\Application\chrome.exe"),
                ]);
            }
            candidates.extend([
                PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
                PathBuf::from(r"C:\Program Files\Microsoft\Edge\Application\msedge.exe"),
                PathBuf::from(r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"),
                PathBuf::from(
                    r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
                ),
                PathBuf::from(
                    r"C:\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe",
                ),
            ]);
            candidates
        }
    }
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
    }
}
