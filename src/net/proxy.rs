use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProxyReport {
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
}

pub(crate) fn proxy_report() -> ProxyReport {
    if let Some(proxy) = environment_proxy() {
        return ProxyReport {
            source: "environment",
            address: Some(redact_proxy(&proxy)),
        };
    }

    #[cfg(windows)]
    if let Some(config) = windows_system_proxy_config() {
        return ProxyReport {
            source: "windows_system",
            address: Some(config.redacted_summary()),
        };
    }

    #[cfg(windows)]
    if let Some(address) = windows_auto_proxy_summary() {
        return ProxyReport {
            source: "windows_auto_unsupported",
            address,
        };
    }

    ProxyReport {
        source: "direct_or_auto",
        address: None,
    }
}

pub(crate) fn apply_to_client_builder(
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::ClientBuilder, crate::core::CliError> {
    #[cfg(windows)]
    if environment_proxy().is_none()
        && let Some(config) = windows_system_proxy_config()
    {
        let no_proxy = config
            .bypass
            .as_deref()
            .and_then(reqwest::NoProxy::from_string);
        let mut builder = builder;
        if let Some(address) = config.http.as_deref() {
            let mut proxy = reqwest::Proxy::http(address).map_err(invalid_windows_proxy)?;
            proxy = proxy.no_proxy(no_proxy.clone());
            builder = builder.proxy(proxy);
        }
        if let Some(address) = config.https.as_deref() {
            let mut proxy = reqwest::Proxy::https(address).map_err(invalid_windows_proxy)?;
            proxy = proxy.no_proxy(no_proxy);
            builder = builder.proxy(proxy);
        }
        return Ok(builder);
    }
    Ok(builder)
}

#[cfg(windows)]
fn invalid_windows_proxy(error: reqwest::Error) -> crate::core::CliError {
    crate::core::CliError::Config(format!(
        "invalid Windows system proxy configuration: {error}"
    ))
}

/// `self_update` uses ureq, whose Windows registry proxy support is not
/// enabled by its public feature set. Bridge the same Internet Settings proxy
/// into the conventional env variables only for the lifetime of an update.
pub(crate) struct UpdateProxyEnvGuard {
    #[cfg(windows)]
    installed: bool,
    #[cfg(windows)]
    installed_no_proxy: bool,
}

impl UpdateProxyEnvGuard {
    pub(crate) fn activate() -> Self {
        #[cfg(windows)]
        {
            if environment_proxy().is_none()
                && let Some(config) = windows_system_proxy_config()
                && let Some(proxy) = config.https
            {
                // SAFETY: update runs as a single top-level CLI operation. The
                // guard restores the variable before returning, and no other
                // sunox task is launched concurrently in this process.
                unsafe {
                    std::env::set_var("HTTPS_PROXY", proxy);
                }
                let installed_no_proxy = std::env::var_os("NO_PROXY").is_none()
                    && std::env::var_os("no_proxy").is_none()
                    && config.bypass.is_some();
                if installed_no_proxy {
                    // SAFETY: same scoped environment mutation described above.
                    unsafe {
                        std::env::set_var("NO_PROXY", config.bypass.expect("checked above"));
                    }
                }
                return Self {
                    installed: true,
                    installed_no_proxy,
                };
            }
            Self {
                installed: false,
                installed_no_proxy: false,
            }
        }

        #[cfg(not(windows))]
        Self {}
    }
}

impl Drop for UpdateProxyEnvGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.installed {
            // SAFETY: see `activate`; this only reverts variables installed by
            // this guard during the same top-level update command.
            unsafe {
                std::env::remove_var("HTTPS_PROXY");
                if self.installed_no_proxy {
                    std::env::remove_var("NO_PROXY");
                }
            }
        }
    }
}

fn environment_proxy() -> Option<String> {
    [
        "HTTPS_PROXY",
        "https_proxy",
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .into_iter()
    .find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn redact_proxy(proxy: &str) -> String {
    let normalized = with_proxy_scheme(proxy);
    let Ok(mut url) = reqwest::Url::parse(&normalized) else {
        return "<invalid proxy address>".into();
    };
    if !url.username().is_empty() {
        let _ = url.set_username("***");
    }
    if url.password().is_some() {
        let _ = url.set_password(Some("***"));
    }
    url.to_string().trim_end_matches('/').to_string()
}

fn with_proxy_scheme(proxy: &str) -> String {
    if proxy.contains("://") {
        proxy.to_string()
    } else {
        format!("http://{proxy}")
    }
}

#[cfg(windows)]
struct WindowsSystemProxy {
    http: Option<String>,
    https: Option<String>,
    bypass: Option<String>,
}

#[cfg(windows)]
impl WindowsSystemProxy {
    fn redacted_summary(&self) -> String {
        match (&self.http, &self.https) {
            (Some(http), Some(https)) if http == https => redact_proxy(http),
            (http, https) => [
                http.as_deref()
                    .map(|address| format!("http={}", redact_proxy(address))),
                https
                    .as_deref()
                    .map(|address| format!("https={}", redact_proxy(address))),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(";"),
        }
    }
}

#[cfg(windows)]
fn windows_system_proxy_config() -> Option<WindowsSystemProxy> {
    if registry_dword("ProxyEnable") != Some(1) {
        return None;
    }
    let configured = registry_string("ProxyServer")?;
    let (http, https) = select_proxy_servers(&configured);
    if http.is_none() && https.is_none() {
        return None;
    }
    let bypass = registry_string("ProxyOverride").and_then(|value| normalize_proxy_bypass(&value));
    Some(WindowsSystemProxy {
        http: http.map(|proxy| with_proxy_scheme(&proxy)),
        https: https.map(|proxy| with_proxy_scheme(&proxy)),
        bypass,
    })
}

#[cfg(any(windows, test))]
fn select_proxy_servers(configured: &str) -> (Option<String>, Option<String>) {
    if configured.contains('=') {
        let entries = configured
            .split(';')
            .filter_map(|entry| entry.split_once('='))
            .map(|(scheme, value)| (scheme.trim(), value.trim()))
            .collect::<Vec<_>>();
        let http = entries
            .iter()
            .find(|(scheme, _)| scheme.eq_ignore_ascii_case("http"))
            .map(|(_, value)| (*value).to_string())
            .filter(|value| !value.is_empty());
        let https = entries
            .iter()
            .find(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
            .map(|(_, value)| (*value).to_string())
            .filter(|value| !value.is_empty());
        (http, https)
    } else {
        let proxy = (!configured.trim().is_empty()).then(|| configured.trim().to_string());
        (proxy.clone(), proxy)
    }
}

#[cfg(windows)]
fn windows_auto_proxy_summary() -> Option<Option<String>> {
    let script = registry_string("AutoConfigURL").filter(|value| !value.trim().is_empty());
    let auto_detect = registry_dword("AutoDetect") == Some(1);
    if script.is_none() && !auto_detect {
        return None;
    }
    Some(script.map(|url| redact_proxy(&url)))
}

#[cfg(any(windows, test))]
fn normalize_proxy_bypass(configured: &str) -> Option<String> {
    let normalized = configured
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty() && !entry.starts_with('<'))
        .map(|entry| entry.strip_prefix("*.").unwrap_or(entry))
        .collect::<Vec<_>>()
        .join(",");
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(windows)]
fn registry_dword(value_name: &str) -> Option<u32> {
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings");
    let name = wide(value_name);
    let mut value = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&mut value as *mut u32).cast::<c_void>(),
            &mut size,
        )
    };
    (status == 0).then_some(value)
}

#[cfg(windows)]
fn registry_string(value_name: &str) -> Option<String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};

    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings");
    let name = wide(value_name);
    let mut size = 0_u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            null_mut(),
            &mut size,
        )
    };
    if status != 0 || size < 2 {
        return None;
    }
    let mut buffer = vec![0_u16; size.div_ceil(2) as usize];
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }
    let length = buffer
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..length]))
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{normalize_proxy_bypass, redact_proxy, select_proxy_servers, with_proxy_scheme};

    #[test]
    fn proxy_addresses_gain_a_scheme_and_hide_credentials() {
        assert_eq!(with_proxy_scheme("127.0.0.1:7890"), "http://127.0.0.1:7890");
        assert_eq!(
            redact_proxy("http://alice:secret@127.0.0.1:7890"),
            "http://***:***@127.0.0.1:7890"
        );
        assert_eq!(
            redact_proxy("http://alice:secret@[invalid"),
            "<invalid proxy address>"
        );
    }

    #[test]
    fn protocol_specific_windows_proxy_preserves_each_scheme() {
        assert_eq!(
            select_proxy_servers("http=127.0.0.1:8080;https=127.0.0.1:8443"),
            (Some("127.0.0.1:8080".into()), Some("127.0.0.1:8443".into()))
        );
        assert_eq!(
            select_proxy_servers("http=127.0.0.1:8080"),
            (Some("127.0.0.1:8080".into()), None)
        );
        assert_eq!(
            select_proxy_servers("127.0.0.1:7890"),
            (Some("127.0.0.1:7890".into()), Some("127.0.0.1:7890".into()))
        );
        assert_eq!(
            normalize_proxy_bypass("localhost;*.example.com;<local>").as_deref(),
            Some("localhost,example.com")
        );
    }
}
