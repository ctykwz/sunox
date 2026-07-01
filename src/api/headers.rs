use reqwest::header::{ACCEPT_LANGUAGE, HeaderMap, USER_AGENT};

use super::SunoClient;
use crate::auth;
use crate::net::http;

impl SunoClient {
    pub(crate) fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        let (jwt, device, browser_environment) = {
            let auth = self.auth.lock().expect("auth mutex poisoned");
            (
                auth.jwt.clone(),
                auth.device_id
                    .clone()
                    .unwrap_or_else(|| "00000000-0000-0000-0000-000000000000".to_string()),
                auth.browser_environment.clone(),
            )
        };

        if let Some(jwt) = jwt
            && let Ok(val) = format!("Bearer {jwt}").parse()
        {
            headers.insert("authorization", val);
        }
        if let Ok(val) = device.parse() {
            headers.insert("device-id", val);
        }
        if let Ok(val) = auth::browser_token().parse() {
            headers.insert("browser-token", val);
        }
        let user_agent = browser_environment
            .as_ref()
            .and_then(|env| env.user_agent.as_deref())
            .unwrap_or(http::BROWSER_USER_AGENT);
        if let Ok(val) = user_agent.parse() {
            headers.insert(USER_AGENT, val);
        }
        if let Some(accept_language) = browser_environment.and_then(|env| env.accept_language)
            && let Ok(val) = accept_language.parse()
        {
            headers.insert(ACCEPT_LANGUAGE, val);
        }
        if let Ok(val) = "https://suno.com".parse() {
            headers.insert("origin", val);
        }
        if let Ok(val) = "https://suno.com/".parse() {
            headers.insert("referer", val);
        }
        headers
    }
}
