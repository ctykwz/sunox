use std::time::Duration;

use reqwest::Client;

use crate::core::CliError;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36";

pub fn browser_client() -> Result<Client, CliError> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(BROWSER_USER_AGENT)
        .build()
        .map_err(|e| CliError::Config(format!("HTTP client: {e}")))
}

pub fn default_client() -> Result<Client, CliError> {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| CliError::Config(format!("HTTP client: {e}")))
}
