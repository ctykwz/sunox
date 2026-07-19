use std::sync::Mutex;

use reqwest::Client;

use crate::auth::AuthState;
use crate::core::CliError;
use crate::net::http;

pub(crate) const BASE_URL: &str = "https://studio-api-prod.suno.com";

pub struct SunoClient {
    pub(crate) client: Client,
    base_url: String,
    /// Auth state behind a sync mutex so `&self` methods can transparently
    /// refresh the JWT mid-request when Suno returns
    /// `Token validation failed.` (their server-side staleness threshold
    /// kicks in well before the JWT's own `exp` claim). The lock is only
    /// held briefly to read/clone auth fields; never across awaits.
    pub(crate) auth: Mutex<AuthState>,
    pub(crate) device_override: Mutex<Option<String>>,
}

impl SunoClient {
    /// Create a new client. If JWT is expired but we have a Clerk cookie,
    /// auto-refresh the JWT transparently.
    pub async fn new_with_refresh(mut auth: AuthState) -> Result<Self, CliError> {
        let client = http::browser_client()?;
        super::auth_retry::refresh_state_if_needed(&client, &mut auth).await?;

        Ok(Self {
            client,
            base_url: BASE_URL.to_string(),
            auth: Mutex::new(auth),
            device_override: Mutex::new(None),
        })
    }

    /// Build a client for login verification without refreshing or persisting
    /// authentication. Interactive login uses this while its browser window is
    /// still open, so only a token accepted by the Suno API completes login.
    pub(crate) fn new_for_auth_validation(auth: AuthState) -> Result<Self, CliError> {
        Ok(Self {
            client: http::browser_client()?,
            base_url: BASE_URL.to_string(),
            auth: Mutex::new(auth),
            device_override: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(base_url: String, auth: AuthState) -> Result<Self, CliError> {
        Ok(Self {
            client: http::browser_client()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth: Mutex::new(auth),
            device_override: Mutex::new(None),
        })
    }

    pub(crate) fn auth_state_snapshot(&self) -> AuthState {
        self.auth.lock().expect("auth mutex poisoned").clone()
    }

    pub(crate) fn set_device_id(&self, device_id: String) {
        self.auth.lock().expect("auth mutex poisoned").device_id = Some(device_id.clone());
        *self
            .device_override
            .lock()
            .expect("device override mutex poisoned") = Some(device_id);
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub(crate) fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.get(self.url(path)).headers(self.headers())
    }

    pub(crate) fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.post(self.url(path)).headers(self.headers())
    }

    pub(crate) fn patch(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.patch(self.url(path)).headers(self.headers())
    }

    pub(crate) fn put(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.put(self.url(path)).headers(self.headers())
    }

    pub(crate) fn delete(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.delete(self.url(path)).headers(self.headers())
    }
}
