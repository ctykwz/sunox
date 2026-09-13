use std::sync::Mutex;
use std::time::Instant;

use reqwest::Client;
use serde::de::DeserializeOwned;

use crate::auth::AuthState;
use crate::core::CliError;
use crate::net::http;

pub(crate) const BASE_URL: &str = "https://studio-api-prod.suno.com";

pub struct SunoClient {
    pub(crate) client: Client,
    no_redirect_client: Client,
    http1_read_client: Client,
    pub(crate) clerk_client: Client,
    base_url: String,
    /// Auth state behind a sync mutex so `&self` methods can transparently
    /// refresh the JWT mid-request when Suno returns
    /// `Token validation failed.` (their server-side staleness threshold
    /// kicks in well before the JWT's own `exp` claim). The lock is only
    /// held briefly to read/clone auth fields; never across awaits.
    pub(crate) auth: Mutex<AuthState>,
    pub(crate) auth_refresh: tokio::sync::Mutex<()>,
    pub(crate) device_override: Mutex<Option<String>>,
    /// Live clients must validate even when no earlier command preflight ran.
    /// Low-level endpoint fixtures can exercise the transport contract alone.
    pub(crate) requires_initial_mutation_preflight: bool,
    pub(crate) mutation_auth_preflight_at: Mutex<Option<Instant>>,
}

impl SunoClient {
    /// Create a new client. If JWT is expired but we have a Clerk cookie,
    /// auto-refresh the JWT transparently.
    pub async fn new_with_refresh(mut auth: AuthState) -> Result<Self, CliError> {
        let client = http::browser_client()?;
        let clerk_client = http::clerk_client()?;
        super::auth_retry::refresh_state_if_needed(&clerk_client, &mut auth).await?;

        Ok(Self {
            client,
            no_redirect_client: http::browser_no_redirect_client()?,
            http1_read_client: http::browser_http1_client()?,
            clerk_client,
            base_url: api_base_url(),
            auth: Mutex::new(auth),
            auth_refresh: tokio::sync::Mutex::new(()),
            device_override: Mutex::new(None),
            requires_initial_mutation_preflight: true,
            mutation_auth_preflight_at: Mutex::new(None),
        })
    }

    /// Build a client for login verification without refreshing or persisting
    /// authentication. Interactive login uses this while its browser window is
    /// still open, so only a token accepted by the Suno API completes login.
    pub(crate) fn new_for_auth_validation(auth: AuthState) -> Result<Self, CliError> {
        Ok(Self {
            client: http::browser_client()?,
            no_redirect_client: http::browser_no_redirect_client()?,
            http1_read_client: http::browser_http1_client()?,
            clerk_client: http::clerk_client()?,
            base_url: BASE_URL.to_string(),
            auth: Mutex::new(auth),
            auth_refresh: tokio::sync::Mutex::new(()),
            device_override: Mutex::new(None),
            requires_initial_mutation_preflight: true,
            mutation_auth_preflight_at: Mutex::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(base_url: String, auth: AuthState) -> Result<Self, CliError> {
        Ok(Self {
            client: http::browser_client()?,
            no_redirect_client: http::browser_no_redirect_client()?,
            http1_read_client: http::browser_http1_client()?,
            clerk_client: http::clerk_client()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth: Mutex::new(auth),
            auth_refresh: tokio::sync::Mutex::new(()),
            device_override: Mutex::new(None),
            requires_initial_mutation_preflight: false,
            mutation_auth_preflight_at: Mutex::new(None),
        })
    }

    pub(crate) fn auth_state_snapshot(&self) -> AuthState {
        self.auth.lock().expect("auth mutex poisoned").clone()
    }

    pub(crate) fn ensure_active_account(&self) -> Result<(), CliError> {
        #[cfg(test)]
        if !self.requires_initial_mutation_preflight {
            return Ok(());
        }
        let saved = AuthState::load().map_err(|error| match error {
            CliError::AuthMissing => CliError::AuthChanged,
            error => error,
        })?;
        if !self.auth_state_snapshot().matches_account_material(&saved) {
            return Err(CliError::AuthChanged);
        }
        Ok(())
    }

    pub(crate) fn authenticated_user_id(&self) -> Option<String> {
        self.auth_state_snapshot().account_user_id()
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

    pub(crate) fn post_without_redirect(&self, path: &str) -> reqwest::RequestBuilder {
        self.no_redirect_client
            .post(self.url(path))
            .headers(self.headers())
    }

    pub(crate) fn patch_without_redirect(&self, path: &str) -> reqwest::RequestBuilder {
        self.no_redirect_client
            .patch(self.url(path))
            .headers(self.headers())
    }

    pub(crate) fn put_without_redirect(&self, path: &str) -> reqwest::RequestBuilder {
        self.no_redirect_client
            .put(self.url(path))
            .headers(self.headers())
    }

    pub(crate) fn delete_without_redirect(&self, path: &str) -> reqwest::RequestBuilder {
        self.no_redirect_client
            .delete(self.url(path))
            .headers(self.headers())
    }

    /// Retry explicitly idempotent reads after transient resets observed with
    /// both normal negotiation and forced HTTP/1.1. The alternate transport is
    /// a bounded recovery attempt, not a route-specific protocol claim. Never
    /// use this for account mutations.
    pub(crate) async fn read_json_with_transport_retry<T>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T, CliError>
    where
        T: DeserializeOwned,
    {
        let request = request.build()?;
        if request.method() != reqwest::Method::GET {
            return Err(CliError::Config(format!(
                "idempotent read fallback cannot send {} requests",
                request.method()
            )));
        }
        let http1_request = request.try_clone();
        let final_default_request = request.try_clone();

        match self.execute_json_read(&self.client, request).await {
            Ok(value) => Ok(value),
            Err(JsonReadError::Fatal(error)) => Err(error),
            Err(JsonReadError::Retryable(primary_error)) => {
                if let Some(http1_request) = http1_request {
                    match self
                        .execute_json_read(&self.http1_read_client, http1_request)
                        .await
                    {
                        Ok(value) => return Ok(value),
                        Err(JsonReadError::Fatal(error)) => return Err(error),
                        Err(JsonReadError::Retryable(_)) => {}
                    }
                }
                if let Some(final_default_request) = final_default_request {
                    return self
                        .execute_json_read(&self.client, final_default_request)
                        .await
                        .map_err(JsonReadError::into_cli_error);
                }
                Err(primary_error)
            }
        }
    }

    async fn execute_json_read<T>(
        &self,
        client: &Client,
        request: reqwest::Request,
    ) -> Result<T, JsonReadError>
    where
        T: DeserializeOwned,
    {
        let response = client
            .execute(request)
            .await
            .map_err(|error| JsonReadError::Retryable(error.into()))?;
        let response = self
            .check_response(response)
            .await
            .map_err(JsonReadError::Fatal)?;
        match response.json::<T>().await {
            Ok(value) => Ok(value),
            Err(error) if is_response_body_transport_error(&error) => {
                Err(JsonReadError::Retryable(error.into()))
            }
            Err(error) => Err(JsonReadError::Fatal(error.into())),
        }
    }
}

fn api_base_url() -> String {
    #[cfg(debug_assertions)]
    {
        debug_test_base_url(std::env::var("SUNOX_TEST_API_BASE_URL").ok().as_deref())
    }

    #[cfg(not(debug_assertions))]
    {
        BASE_URL.to_string()
    }
}

#[cfg(debug_assertions)]
fn debug_test_base_url(candidate: Option<&str>) -> String {
    let Some(candidate) = candidate.filter(|value| !value.trim().is_empty()) else {
        return BASE_URL.to_string();
    };
    let Ok(url) = reqwest::Url::parse(candidate) else {
        return BASE_URL.to_string();
    };
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if url.scheme() != "http"
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return BASE_URL.to_string();
    }
    candidate.trim_end_matches('/').to_string()
}

fn is_response_body_transport_error(error: &reqwest::Error) -> bool {
    if error.is_body() {
        return true;
    }

    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        if cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_body)
        {
            return true;
        }
        source = cause.source();
    }
    false
}

enum JsonReadError {
    Retryable(CliError),
    Fatal(CliError),
}

impl JsonReadError {
    fn into_cli_error(self) -> CliError {
        match self {
            Self::Retryable(error) | Self::Fatal(error) => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BASE_URL, debug_test_base_url};

    #[test]
    fn debug_api_override_accepts_only_loopback_http_origins() {
        assert_eq!(
            debug_test_base_url(Some("http://127.0.0.1:43123")),
            "http://127.0.0.1:43123"
        );
        assert_eq!(
            debug_test_base_url(Some("http://[::1]:43123/")),
            "http://[::1]:43123"
        );
        assert_eq!(
            debug_test_base_url(Some("https://studio-api-prod.suno.com")),
            BASE_URL
        );
        assert_eq!(debug_test_base_url(Some("not a url")), BASE_URL);
    }
}
