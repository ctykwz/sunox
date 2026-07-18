use std::time::Duration;

use reqwest::Client;

use crate::core::CliError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const TRANSFER_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
#[cfg(target_os = "macos")]
pub(crate) const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";
#[cfg(target_os = "windows")]
pub(crate) const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";
#[cfg(target_os = "linux")]
pub(crate) const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub(crate) const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko) Chrome/149.0.0.0 Safari/537.36";
pub(crate) const BROWSER_ACCEPT_LANGUAGE: &str = "en";

pub fn browser_client() -> Result<Client, CliError> {
    crate::net::proxy::apply_to_client_builder(
        Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(BROWSER_USER_AGENT),
    )?
    .build()
    .map_err(|e| CliError::Config(format!("HTTP client: {e}")))
}

/// CDN media can legitimately take longer than an API response. Keep the
/// connection bounded but do not impose Reqwest's total-body deadline.
pub fn download_client() -> Result<Client, CliError> {
    crate::net::proxy::apply_to_client_builder(Client::builder().connect_timeout(REQUEST_TIMEOUT))?
        .build()
        .map_err(|e| CliError::Config(format!("HTTP client: {e}")))
}

/// Presigned uploads can be much larger than API payloads. Keep connection,
/// idle-read, and total deadlines explicit without inheriting the API's 30s
/// total timeout.
pub fn transfer_client() -> Result<Client, CliError> {
    crate::net::proxy::apply_to_client_builder(
        Client::builder()
            .connect_timeout(REQUEST_TIMEOUT)
            .read_timeout(TRANSFER_IDLE_TIMEOUT)
            .timeout(TRANSFER_TIMEOUT),
    )?
    .build()
    .map_err(|e| CliError::Config(format!("HTTP transfer client: {e}")))
}

/// Browser control endpoints are always owned loopback services. Never allow
/// environment or operating-system proxy settings to intercept CDP traffic.
pub fn loopback_client() -> Result<Client, CliError> {
    Client::builder()
        .no_proxy()
        .build()
        .map_err(|e| CliError::Config(format!("loopback HTTP client: {e}")))
}
