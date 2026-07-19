use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserClientHints {
    pub sec_ch_ua: String,
    pub sec_ch_ua_mobile: String,
    pub sec_ch_ua_platform: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserEnvironment {
    /// Browser or login flow that produced the stored auth material. This is
    /// diagnostic metadata, not a substitute for runtime browser headers.
    pub browser_source: Option<String>,
    pub user_agent: Option<String>,
    pub accept_language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_hints: Option<BrowserClientHints>,
}

#[derive(Debug, Clone)]
pub struct BrowserAuth {
    pub clerk_client_cookie: String,
    pub cookie_header: String,
    pub device_id: Option<String>,
    pub browser_environment: Option<BrowserEnvironment>,
}
