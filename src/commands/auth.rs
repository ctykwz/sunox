use std::future::Future;

use crate::api::SunoClient;
use crate::app::AppContext;
use crate::auth::{self, AuthState, BrowserAuth};
use crate::cli::AuthArgs;
use crate::core::CliError;

pub async fn run(args: AuthArgs, _ctx: &AppContext) -> Result<(), CliError> {
    if args.logout {
        run_logout_with_cleanup(AuthState::delete, auth::delete_interactive_browser_profile)?;
        return Ok(());
    }

    let mut state = match AuthState::load() {
        Ok(s) => s,
        Err(CliError::AuthMissing) => AuthState::default(),
        Err(e) => return Err(e),
    };

    let has_explicit_auth_input =
        args.login || args.refresh || args.jwt.is_some() || args.cookie.is_some();
    let should_login = args.login
        || (!has_explicit_auth_input && state.jwt.is_none() && state.clerk_client_cookie.is_none());

    if args.refresh {
        let cookie = state.clerk_client_cookie.clone().ok_or_else(|| {
            CliError::Config("no Clerk session cookie stored — run `sunox login` first".into())
        })?;
        let http = reqwest::Client::new();
        eprintln!("Refreshing JWT via Clerk session cookie...");
        let (session_id, jwt) = if let Some(session_id) = state.session_id.clone() {
            (
                session_id.clone(),
                auth::clerk_refresh_jwt(&http, &cookie, &session_id).await?,
            )
        } else {
            auth::clerk_token_exchange(&http, &cookie).await?
        };
        state.session_id = Some(session_id);
        state.jwt = Some(jwt);
        state.save()?;
        eprintln!("JWT refreshed successfully");
    } else if should_login {
        eprintln!("Extracting Suno session from your browser...");
        let browser_auth = extract_browser_auth_with_fallback(
            auth::extract_browser_auth,
            auth::extract_interactive_browser_auth,
        )
        .await?;

        let http = reqwest::Client::new();
        eprintln!("Exchanging for access token via Clerk...");
        let (session_id, jwt) =
            auth::clerk_token_exchange(&http, &browser_auth.clerk_client_cookie).await?;

        state.cookie = Some(browser_auth.cookie_header);
        state.clerk_client_cookie = Some(browser_auth.clerk_client_cookie);
        state.session_id = Some(session_id);
        state.jwt = Some(jwt);
        state.device_id = browser_auth
            .device_id
            .or(state.device_id)
            .or_else(|| Some(uuid::Uuid::new_v4().to_string()));
    } else if let Some(cookie) = args.cookie.as_deref() {
        let browser_auth = auth::normalize_cookie_input(cookie)?;
        let http = reqwest::Client::new();
        eprintln!("Exchanging cookie for access token...");
        let (session_id, jwt) =
            auth::clerk_token_exchange(&http, &browser_auth.clerk_client_cookie).await?;

        state.cookie = Some(browser_auth.cookie_header);
        state.clerk_client_cookie = Some(browser_auth.clerk_client_cookie);
        state.session_id = Some(session_id);
        state.jwt = Some(jwt);
        state.device_id = browser_auth
            .device_id
            .or(state.device_id)
            .or_else(|| Some(uuid::Uuid::new_v4().to_string()));
    } else if let Some(jwt) = args.jwt.clone() {
        state.jwt = Some(jwt);
        if state.device_id.is_none() {
            state.device_id = Some(uuid::Uuid::new_v4().to_string());
        }
    } else {
        eprintln!("Checking existing authentication...");
    }

    if let Some(device) = args.device.as_ref() {
        state.device_id = Some(device.clone());
    }

    let should_save_after_verify = args.refresh
        || should_login
        || args.cookie.is_some()
        || args.jwt.is_some()
        || args.device.is_some();
    let client = SunoClient::new_with_refresh(state.clone()).await?;
    let info = client.billing_info().await?;
    if should_save_after_verify {
        state.save()?;
    }
    eprintln!(
        "Authenticated! Plan: {}, Credits: {}",
        info.plan.name, info.total_credits_left
    );
    Ok(())
}

fn run_logout_with_cleanup<D, P>(
    delete_auth_state: D,
    delete_interactive_profile: P,
) -> Result<(), CliError>
where
    D: FnOnce() -> Result<(), CliError>,
    P: FnOnce() -> Result<(), CliError>,
{
    delete_auth_state()?;
    delete_interactive_profile()?;
    eprintln!("Logged out; removed stored Suno authentication");
    Ok(())
}

async fn extract_browser_auth_with_fallback<C, I, Fut>(
    browser_cookie_probe: C,
    interactive_login: I,
) -> Result<BrowserAuth, CliError>
where
    C: FnOnce() -> Result<BrowserAuth, CliError>,
    I: FnOnce() -> Fut,
    Fut: Future<Output = Result<BrowserAuth, CliError>>,
{
    match browser_cookie_probe() {
        Ok(auth) => Ok(auth),
        Err(cookie_error) => {
            eprintln!("Browser cookie extraction failed: {cookie_error}");
            eprintln!("Falling back to interactive browser login...");
            interactive_login().await.map_err(|interactive_error| {
                CliError::Config(format!(
                    "browser cookie extraction failed ({cookie_error}); interactive browser login failed ({interactive_error})"
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    fn auth_with_client(value: &str) -> BrowserAuth {
        BrowserAuth {
            clerk_client_cookie: value.into(),
            cookie_header: format!("__client={value}"),
            device_id: None,
        }
    }

    #[tokio::test]
    async fn login_auth_uses_browser_cookie_when_available() {
        let interactive_called = Cell::new(false);

        let auth = extract_browser_auth_with_fallback(
            || Ok(auth_with_client("browser-cookie")),
            || async {
                interactive_called.set(true);
                Ok(auth_with_client("interactive"))
            },
        )
        .await
        .expect("auth");

        assert_eq!(auth.clerk_client_cookie, "browser-cookie");
        assert!(!interactive_called.get());
    }

    #[tokio::test]
    async fn login_auth_falls_back_to_interactive_browser_when_cookie_probe_fails() {
        let auth = extract_browser_auth_with_fallback(
            || Err(CliError::Config("cookie blocked".into())),
            || async { Ok(auth_with_client("interactive")) },
        )
        .await
        .expect("auth");

        assert_eq!(auth.clerk_client_cookie, "interactive");
    }

    #[test]
    fn logout_removes_stored_auth_and_interactive_browser_profile() {
        let mut deleted_auth = false;
        let mut deleted_profile = false;

        run_logout_with_cleanup(
            || {
                deleted_auth = true;
                Ok(())
            },
            || {
                deleted_profile = true;
                Ok(())
            },
        )
        .expect("logout");

        assert!(deleted_auth);
        assert!(deleted_profile);
    }
}
