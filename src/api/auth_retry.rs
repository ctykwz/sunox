use super::SunoClient;
use crate::auth::{self, AuthState};
use crate::core::CliError;

pub(super) async fn refresh_state_if_needed(
    client: &reqwest::Client,
    auth: &mut AuthState,
) -> Result<(), CliError> {
    auth::refresh_state_if_needed(client, auth).await
}

impl SunoClient {
    /// Refresh the JWT via the stored Clerk session cookie. Used by the
    /// in-process retry path in `with_auth_retry` when Suno's server-side
    /// staleness check fires mid-request despite a still-valid `exp` claim.
    pub(crate) async fn refresh_jwt_after_auth_failure(
        &self,
        failed_jwt: Option<String>,
    ) -> Result<(), CliError> {
        let mut auth = {
            let auth = self.auth.lock().expect("auth mutex poisoned");
            if !should_refresh_after_auth_failure(auth.jwt.as_deref(), failed_jwt.as_deref()) {
                return Ok(());
            }
            auth.clone()
        };
        // Cross-process serialization is handled inside `refresh_state_for_retry`
        // so this retry path does not hold the in-process mutex across await.
        auth::refresh_state_for_retry(&self.client, &mut auth).await?;

        {
            let mut current_auth = self.auth.lock().expect("auth mutex poisoned");
            if should_replace_auth_after_refresh(
                current_auth.jwt.as_deref(),
                failed_jwt.as_deref(),
                auth.jwt.as_deref(),
            ) {
                *current_auth = auth;
            }
        }
        Ok(())
    }

    pub(crate) async fn try_refresh_jwt_for_challenge_recheck(&self) -> Result<bool, CliError> {
        let (request_jwt, has_refresh_material) = {
            let auth = self.auth.lock().expect("auth mutex poisoned");
            (auth.jwt.clone(), auth.clerk_client_cookie.is_some())
        };
        if !has_refresh_material {
            return Ok(false);
        }

        match self.refresh_jwt_after_auth_failure(request_jwt).await {
            Ok(()) => Ok(true),
            Err(CliError::AuthExpired) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn current_jwt(&self) -> Option<String> {
        self.auth.lock().expect("auth mutex poisoned").jwt.clone()
    }

    /// Run an async API call once. If it fails with `AuthExpired`, refresh
    /// the JWT and try a single retry.
    pub(crate) async fn with_auth_retry<F, Fut, T>(&self, mut f: F) -> Result<T, CliError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, CliError>>,
    {
        let request_jwt = self.current_jwt();
        match f().await {
            Err(CliError::AuthExpired) => {
                self.refresh_jwt_after_auth_failure(request_jwt).await?;
                f().await
            }
            other => other,
        }
    }
}

fn should_refresh_after_auth_failure(current_jwt: Option<&str>, failed_jwt: Option<&str>) -> bool {
    current_jwt == failed_jwt
}

fn should_replace_auth_after_refresh(
    current_jwt: Option<&str>,
    failed_jwt: Option<&str>,
    refreshed_jwt: Option<&str>,
) -> bool {
    current_jwt == failed_jwt || current_jwt == refreshed_jwt
}

#[cfg(test)]
mod tests {
    use super::{should_refresh_after_auth_failure, should_replace_auth_after_refresh};

    #[test]
    fn retry_refresh_runs_when_auth_still_matches_failed_jwt() {
        assert!(should_refresh_after_auth_failure(
            Some("old-jwt"),
            Some("old-jwt")
        ));
        assert!(should_refresh_after_auth_failure(None, None));
    }

    #[test]
    fn retry_refresh_skips_when_another_task_already_updated_jwt() {
        assert!(!should_refresh_after_auth_failure(
            Some("new-jwt"),
            Some("old-jwt")
        ));
        assert!(!should_refresh_after_auth_failure(Some("new-jwt"), None));
    }

    #[test]
    fn retry_refresh_does_not_overwrite_newer_auth_state() {
        assert!(should_replace_auth_after_refresh(
            Some("old-jwt"),
            Some("old-jwt"),
            Some("new-jwt")
        ));
        assert!(should_replace_auth_after_refresh(
            Some("new-jwt"),
            Some("old-jwt"),
            Some("new-jwt")
        ));
        assert!(!should_replace_auth_after_refresh(
            Some("newer-jwt"),
            Some("old-jwt"),
            Some("new-jwt")
        ));
    }
}
