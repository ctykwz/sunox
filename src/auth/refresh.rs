use reqwest::Client;

use super::{AuthRefreshLockGuard, AuthState, clerk_refresh_jwt, clerk_token_exchange};
use crate::core::CliError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RefreshMode {
    IfExpired,
    ForceUnlessSavedChanged,
}

pub(crate) async fn refresh_state_if_needed(
    client: &Client,
    auth: &mut AuthState,
) -> Result<(), CliError> {
    if !auth.is_jwt_expired() {
        return Ok(());
    }

    refresh_state_with_lock(client, auth, RefreshMode::IfExpired).await
}

pub(crate) async fn refresh_state_for_retry(
    client: &Client,
    auth: &mut AuthState,
) -> Result<(), CliError> {
    refresh_state_with_lock(client, auth, RefreshMode::ForceUnlessSavedChanged).await
}

pub(crate) async fn refresh_state_explicit(
    client: &Client,
    auth: &mut AuthState,
) -> Result<(), CliError> {
    refresh_state_with_lock(client, auth, RefreshMode::ForceUnlessSavedChanged).await
}

async fn refresh_state_with_lock(
    client: &Client,
    auth: &mut AuthState,
    mode: RefreshMode,
) -> Result<(), CliError> {
    if auth.clerk_client_cookie.is_none() {
        return Err(CliError::AuthExpired);
    }

    let _refresh_guard = AuthRefreshLockGuard::acquire(auth)?;
    if let Ok(saved_auth) = AuthState::load() {
        if !auth.matches_account_material(&saved_auth) {
            return Err(active_auth_changed_error());
        }
        if let Some(reusable_auth) = reusable_saved_auth_after_lock(auth, saved_auth, mode) {
            *auth = reusable_auth;
            return Ok(());
        }
    }
    let refresh_origin = auth.clone();

    if let (Some(cookie), Some(session_id)) = (&auth.clerk_client_cookie, &auth.session_id) {
        eprintln!("{}", refresh_with_session_message(mode));
        match clerk_refresh_jwt(client, cookie, session_id).await {
            Ok(jwt) => {
                auth.jwt = Some(jwt);
                auth.save_after_refresh(&refresh_origin)?;
                eprintln!("JWT refreshed successfully");
                Ok(())
            }
            Err(e) => {
                eprintln!("JWT refresh failed: {e}");
                Err(CliError::AuthExpired)
            }
        }
    } else if let Some(cookie) = &auth.clerk_client_cookie {
        eprintln!("{}", recover_session_message(mode));
        match clerk_token_exchange(client, cookie).await {
            Ok((session_id, jwt)) => {
                auth.session_id = Some(session_id);
                auth.jwt = Some(jwt);
                auth.save_after_refresh(&refresh_origin)?;
                eprintln!("JWT refreshed successfully");
                Ok(())
            }
            Err(e) => {
                eprintln!("JWT refresh failed: {e}");
                Err(CliError::AuthExpired)
            }
        }
    } else {
        Err(CliError::AuthExpired)
    }
}

fn active_auth_changed_error() -> CliError {
    CliError::AuthChanged
}

fn reusable_saved_auth_after_lock(
    current_auth: &AuthState,
    saved_auth: AuthState,
    mode: RefreshMode,
) -> Option<AuthState> {
    if saved_auth.is_jwt_expired() {
        return None;
    }
    if !current_auth.matches_account_material(&saved_auth) {
        return None;
    }

    match mode {
        RefreshMode::IfExpired => Some(saved_auth),
        RefreshMode::ForceUnlessSavedChanged => {
            if saved_auth.jwt != current_auth.jwt {
                Some(saved_auth)
            } else {
                None
            }
        }
    }
}

fn refresh_with_session_message(mode: RefreshMode) -> &'static str {
    match mode {
        RefreshMode::IfExpired => "JWT expired, refreshing via Clerk...",
        RefreshMode::ForceUnlessSavedChanged => "Refreshing JWT via Clerk session cookie...",
    }
}

fn recover_session_message(mode: RefreshMode) -> &'static str {
    match mode {
        RefreshMode::IfExpired => "JWT expired, recovering Clerk session...",
        RefreshMode::ForceUnlessSavedChanged => "Recovering Clerk session...",
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;

    use crate::auth::AuthState;

    use super::{RefreshMode, reusable_saved_auth_after_lock};

    fn jwt(exp: u64, subject: &str, marker: &str) -> String {
        let header = BASE64URL.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let claims = BASE64URL.encode(format!(
            r#"{{"sub":"{subject}","exp":{exp},"jti":"{marker}"}}"#
        ));
        format!("{header}.{claims}.signature")
    }

    fn auth_with_jwt(jwt: String) -> AuthState {
        AuthState {
            jwt: Some(jwt),
            clerk_client_cookie: Some("client-cookie".into()),
            session_id: Some("session-id".into()),
            ..Default::default()
        }
    }

    #[test]
    fn expired_startup_refresh_reuses_fresh_saved_auth_after_lock() {
        let current = auth_with_jwt(jwt(1, "user-a", "old"));
        let saved = auth_with_jwt(jwt(4_102_444_800, "user-a", "new"));

        let reusable =
            reusable_saved_auth_after_lock(&current, saved.clone(), RefreshMode::IfExpired)
                .expect("saved auth should be reusable");

        assert_eq!(reusable.jwt, saved.jwt);
    }

    #[test]
    fn forced_refresh_reuses_only_a_different_fresh_saved_jwt() {
        let current = auth_with_jwt(jwt(4_102_444_800, "user-a", "old"));
        let same = auth_with_jwt(current.jwt.clone().expect("current jwt"));
        let newer = auth_with_jwt(jwt(4_102_444_800, "user-a", "new"));

        assert!(
            reusable_saved_auth_after_lock(&current, same, RefreshMode::ForceUnlessSavedChanged)
                .is_none()
        );
        assert_eq!(
            reusable_saved_auth_after_lock(
                &current,
                newer.clone(),
                RefreshMode::ForceUnlessSavedChanged,
            )
            .expect("newer saved auth")
            .jwt,
            newer.jwt
        );
    }

    #[test]
    fn refresh_does_not_reuse_fresh_auth_from_different_account() {
        let current = AuthState {
            jwt: Some(jwt(1, "user-a", "old")),
            session_id: Some("session-a".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };
        let saved = AuthState {
            jwt: Some(jwt(4_102_444_800, "user-b", "new")),
            session_id: Some("session-a".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };

        assert!(reusable_saved_auth_after_lock(&current, saved, RefreshMode::IfExpired).is_none());
    }

    #[test]
    fn refresh_can_reuse_recovered_session_when_current_has_no_jwt() {
        let current = AuthState {
            session_id: Some("session-a".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };
        let saved = AuthState {
            jwt: Some(jwt(4_102_444_800, "user-a", "new")),
            session_id: Some("session-a".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };

        assert_eq!(
            reusable_saved_auth_after_lock(&current, saved.clone(), RefreshMode::IfExpired)
                .expect("same session should be reusable")
                .jwt,
            saved.jwt
        );
    }
}
