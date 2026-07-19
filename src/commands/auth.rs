use std::future::Future;
use std::io::Read;

use crate::api::SunoClient;
use crate::app::AppContext;
use crate::auth::{self, AuthState, BrowserAuth};
use crate::cli::AuthArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn run(args: AuthArgs, ctx: &AppContext) -> Result<(), CliError> {
    if args.logout {
        run_logout_with_cleanup(
            AuthState::delete,
            auth::delete_interactive_browser_profile,
            crate::captcha::delete_legacy_browser_profile,
        )?;
        match ctx.fmt {
            OutputFormat::Json => output::json::success(serde_json::json!({
                "logged_out": true,
                "stored_auth_removed": true,
            })),
            OutputFormat::Table => {
                eprintln!("Logged out; removed stored Suno authentication");
            }
        }
        return Ok(());
    }

    let original_state = match AuthState::load() {
        Ok(state) => Some(state),
        Err(CliError::AuthMissing) => None,
        Err(e) => return Err(e),
    };
    let mut state = original_state.clone().unwrap_or_default();
    let mut environment_recovery_attempted = false;
    let jwt_input = read_secret_input(args.jwt.clone(), args.jwt_stdin, "JWT")?;
    let cookie_input = read_secret_input(args.cookie.clone(), args.cookie_stdin, "Clerk cookie")?;

    let has_explicit_auth_input =
        args.login || args.refresh || jwt_input.is_some() || cookie_input.is_some();
    let should_login = args.login
        || (!has_explicit_auth_input && state.jwt.is_none() && state.clerk_client_cookie.is_none());

    if args.refresh {
        environment_recovery_attempted = true;
        let recovery_origin = state.clone();
        if auth::recover_auth_state_environment(&mut state).await? {
            state.save_if_unchanged(Some(&recovery_origin))?;
        }
        state.clerk_client_cookie.as_ref().ok_or_else(|| {
            CliError::Config("no Clerk session cookie stored — run `sunox login` first".into())
        })?;
        let http = crate::net::http::browser_client()?;
        auth::refresh_state_explicit(&http, &mut state).await?;
    } else if should_login {
        eprintln!("Extracting Suno session from your browser...");
        let mut login = extract_browser_auth_with_fallback(
            auth::extract_browser_auth,
            auth::extract_interactive_browser_auth,
        )
        .await?;
        auth::enrich_browser_auth_environment(&mut login.browser_auth).await?;
        environment_recovery_attempted = true;

        let (session_id, jwt) = match login.verified_clerk {
            Some(verified) => (verified.session_id, verified.jwt),
            None => {
                let http = crate::net::http::browser_client()?;
                eprintln!("Exchanging for access token via Clerk...");
                auth::clerk_token_exchange(
                    &http,
                    &login.browser_auth.clerk_client_cookie,
                    login.browser_auth.browser_environment.as_ref(),
                )
                .await?
            }
        };

        store_browser_auth_state(&mut state, login.browser_auth, session_id, jwt);
    } else if let Some(cookie) = cookie_input.as_deref() {
        let mut browser_auth = auth::normalize_cookie_input(cookie)?;
        auth::enrich_browser_auth_environment(&mut browser_auth).await?;
        environment_recovery_attempted = true;
        let http = crate::net::http::browser_client()?;
        eprintln!("Exchanging cookie for access token...");
        let (session_id, jwt) = auth::clerk_token_exchange(
            &http,
            &browser_auth.clerk_client_cookie,
            browser_auth.browser_environment.as_ref(),
        )
        .await?;

        store_browser_auth_state(&mut state, browser_auth, session_id, jwt);
    } else if let Some(jwt) = jwt_input.as_ref() {
        store_direct_jwt_state(&mut state, jwt.clone());
    } else {
        eprintln!("Checking existing authentication...");
    }

    let recovery_origin = state.clone();
    let environment_recovered = if environment_recovery_attempted {
        false
    } else {
        auth::recover_auth_state_environment(&mut state).await?
    };
    let recovery_can_be_saved_before_verify =
        environment_recovered && !should_login && cookie_input.is_none() && jwt_input.is_none();
    if recovery_can_be_saved_before_verify {
        state.save_if_unchanged(Some(&recovery_origin))?;
    }

    let save_origin = if recovery_can_be_saved_before_verify || args.refresh {
        Some(state.clone())
    } else {
        original_state.clone()
    };
    let should_save_after_verify = args.refresh
        || should_login
        || cookie_input.is_some()
        || args.jwt.is_some()
        || args.jwt_stdin
        || args.device.is_some()
        || (environment_recovered && !recovery_can_be_saved_before_verify);
    let client = SunoClient::new_with_refresh(state.clone()).await?;
    if let Some(device) = args.device.as_ref() {
        client.set_device_id(device.clone());
    }
    let info = client.billing_info().await?;
    if should_save_after_verify {
        let verified = verified_auth_state(&client);
        if args.device.is_some() && !should_login && cookie_input.is_none() && jwt_input.is_none() {
            verified.save_device_id_for_active_account()?;
        } else {
            verified.save_if_unchanged(save_origin.as_ref())?;
        }
    }
    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "authenticated": true,
            "plan": info.plan.name,
            "credits": info.total_credits_left,
        })),
        OutputFormat::Table => eprintln!(
            "Authenticated! Plan: {}, Credits: {}",
            info.plan.name, info.total_credits_left
        ),
    }
    Ok(())
}

fn verified_auth_state(client: &SunoClient) -> AuthState {
    client.auth_state_snapshot()
}

fn run_logout_with_cleanup<D, P, C>(
    delete_auth_state: D,
    delete_interactive_profile: P,
    delete_captcha_profile: C,
) -> Result<(), CliError>
where
    D: FnOnce() -> Result<(), CliError>,
    P: FnOnce() -> Result<(), CliError>,
    C: FnOnce() -> Result<(), CliError>,
{
    delete_auth_state()?;
    delete_interactive_profile()?;
    delete_captcha_profile()?;
    Ok(())
}

fn read_secret_input(
    value: Option<String>,
    from_stdin: bool,
    label: &str,
) -> Result<Option<String>, CliError> {
    let value = if from_stdin {
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input)?;
        Some(input)
    } else {
        value
    };
    value
        .map(|value| {
            let value = value.trim().to_string();
            if value.is_empty() {
                Err(CliError::Config(format!("{label} must not be empty")))
            } else {
                Ok(value)
            }
        })
        .transpose()
}

async fn extract_browser_auth_with_fallback<C, I, Fut>(
    browser_cookie_probe: C,
    interactive_login: I,
) -> Result<LoginAuth, CliError>
where
    C: FnOnce() -> Result<BrowserAuth, CliError>,
    I: FnOnce() -> Fut,
    Fut: Future<Output = Result<(BrowserAuth, String, String), CliError>>,
{
    match browser_cookie_probe() {
        Ok(browser_auth) => Ok(LoginAuth {
            browser_auth,
            verified_clerk: None,
        }),
        Err(cookie_error) => {
            eprintln!("Browser cookie extraction failed: {cookie_error}");
            eprintln!("Falling back to interactive browser login...");
            let (browser_auth, session_id, jwt) =
                interactive_login().await.map_err(|interactive_error| {
                    CliError::Config(format!(
                        "browser cookie extraction failed ({cookie_error}); interactive browser login failed ({interactive_error})"
                    ))
                })?;
            Ok(LoginAuth {
                browser_auth,
                verified_clerk: Some(VerifiedClerkSession { session_id, jwt }),
            })
        }
    }
}

struct LoginAuth {
    browser_auth: BrowserAuth,
    verified_clerk: Option<VerifiedClerkSession>,
}

struct VerifiedClerkSession {
    session_id: String,
    jwt: String,
}

fn store_browser_auth_state(
    state: &mut AuthState,
    browser_auth: BrowserAuth,
    session_id: String,
    jwt: String,
) {
    state.cookie = Some(browser_auth.cookie_header);
    state.clerk_client_cookie = Some(browser_auth.clerk_client_cookie);
    state.session_id = Some(session_id);
    state.jwt = Some(jwt);
    state.device_id = browser_auth
        .device_id
        .or_else(|| state.device_id.take())
        .or_else(|| Some(uuid::Uuid::new_v4().to_string()));
    state.browser_environment = browser_auth.browser_environment;
}

fn store_direct_jwt_state(state: &mut AuthState, jwt: String) {
    state.jwt = Some(jwt);
    state.cookie = None;
    state.clerk_client_cookie = None;
    state.session_id = None;
    state.device_id = Some(uuid::Uuid::new_v4().to_string());
    state.browser_environment = None;
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::api::SunoClient;
    use crate::auth::BrowserEnvironment;

    use super::*;

    fn auth_with_client(value: &str) -> BrowserAuth {
        BrowserAuth {
            clerk_client_cookie: value.into(),
            cookie_header: format!("__client={value}"),
            device_id: None,
            browser_environment: None,
        }
    }

    #[tokio::test]
    async fn login_auth_uses_browser_cookie_when_available() {
        let interactive_called = Cell::new(false);

        let auth = extract_browser_auth_with_fallback(
            || Ok(auth_with_client("browser-cookie")),
            || async {
                interactive_called.set(true);
                Ok((
                    auth_with_client("interactive"),
                    "session".into(),
                    "jwt".into(),
                ))
            },
        )
        .await
        .expect("auth");

        assert_eq!(auth.browser_auth.clerk_client_cookie, "browser-cookie");
        assert!(auth.verified_clerk.is_none());
        assert!(!interactive_called.get());
    }

    #[tokio::test]
    async fn login_auth_preserves_browser_environment_from_cookie_probe() {
        let auth = extract_browser_auth_with_fallback(
            || {
                Ok(BrowserAuth {
                    clerk_client_cookie: "browser-cookie".into(),
                    cookie_header: "__client=browser-cookie".into(),
                    device_id: None,
                    browser_environment: Some(BrowserEnvironment {
                        browser_source: Some("chrome".into()),
                        user_agent: None,
                        accept_language: Some("zh-CN,zh;q=0.9".into()),
                        client_hints: None,
                    }),
                })
            },
            || async {
                Ok((
                    auth_with_client("interactive"),
                    "session".into(),
                    "jwt".into(),
                ))
            },
        )
        .await
        .expect("auth");

        let environment = auth.browser_auth.browser_environment.expect("environment");
        assert_eq!(environment.browser_source.as_deref(), Some("chrome"));
        assert_eq!(
            environment.accept_language.as_deref(),
            Some("zh-CN,zh;q=0.9")
        );
    }

    #[tokio::test]
    async fn login_auth_falls_back_to_interactive_browser_when_cookie_probe_fails() {
        let auth = extract_browser_auth_with_fallback(
            || Err(CliError::Config("cookie blocked".into())),
            || async {
                Ok((
                    auth_with_client("interactive"),
                    "verified-session".into(),
                    "verified-jwt".into(),
                ))
            },
        )
        .await
        .expect("auth");

        assert_eq!(auth.browser_auth.clerk_client_cookie, "interactive");
        let verified = auth.verified_clerk.expect("verified Clerk session");
        assert_eq!(verified.session_id, "verified-session");
        assert_eq!(verified.jwt, "verified-jwt");
    }

    #[test]
    fn browser_auth_state_uses_new_browser_environment() {
        let mut state = AuthState {
            device_id: Some("stored-device".into()),
            browser_environment: Some(BrowserEnvironment {
                browser_source: Some("interactive-browser".into()),
                user_agent: Some("Mozilla/5.0 Test".into()),
                accept_language: Some("en-US,en;q=0.9".into()),
                client_hints: None,
            }),
            ..AuthState::default()
        };

        store_browser_auth_state(
            &mut state,
            BrowserAuth {
                clerk_client_cookie: "client".into(),
                cookie_header: "__client=client".into(),
                device_id: None,
                browser_environment: Some(BrowserEnvironment {
                    browser_source: Some("edge".into()),
                    user_agent: None,
                    accept_language: None,
                    client_hints: None,
                }),
            },
            "session".into(),
            "jwt".into(),
        );

        let environment = state.browser_environment.expect("environment");
        assert_eq!(environment.browser_source.as_deref(), Some("edge"));
        assert_eq!(environment.user_agent, None);
        assert_eq!(environment.accept_language, None);
        assert_eq!(state.device_id.as_deref(), Some("stored-device"));
    }

    #[test]
    fn browser_auth_state_clears_stored_environment_when_new_auth_has_none() {
        let mut state = AuthState {
            browser_environment: Some(BrowserEnvironment {
                browser_source: Some("interactive-browser".into()),
                user_agent: Some("Mozilla/5.0 Test".into()),
                accept_language: Some("en-US,en;q=0.9".into()),
                client_hints: None,
            }),
            ..AuthState::default()
        };

        store_browser_auth_state(
            &mut state,
            BrowserAuth {
                clerk_client_cookie: "client".into(),
                cookie_header: "__client=client".into(),
                device_id: None,
                browser_environment: None,
            },
            "session".into(),
            "jwt".into(),
        );

        assert!(state.browser_environment.is_none());
    }

    #[test]
    fn browser_auth_state_uses_new_device_id_when_available() {
        let mut state = AuthState {
            device_id: Some("stored-device".into()),
            ..AuthState::default()
        };

        store_browser_auth_state(
            &mut state,
            BrowserAuth {
                clerk_client_cookie: "client".into(),
                cookie_header: "__client=client".into(),
                device_id: Some("new-device".into()),
                browser_environment: Some(BrowserEnvironment {
                    browser_source: Some("edge".into()),
                    user_agent: None,
                    accept_language: None,
                    client_hints: None,
                }),
            },
            "session".into(),
            "jwt".into(),
        );

        assert_eq!(state.device_id.as_deref(), Some("new-device"));
    }

    #[test]
    fn direct_jwt_state_clears_stored_refresh_material() {
        let mut state = AuthState {
            jwt: Some("old-jwt".into()),
            cookie: Some("__client=old-client".into()),
            session_id: Some("old-session".into()),
            device_id: Some("old-device".into()),
            browser_environment: Some(BrowserEnvironment {
                browser_source: Some("chrome".into()),
                user_agent: Some("Mozilla/5.0 Old".into()),
                accept_language: Some("en-US,en;q=0.9".into()),
                client_hints: None,
            }),
            clerk_client_cookie: Some("old-client".into()),
        };

        store_direct_jwt_state(&mut state, "new-jwt".into());

        assert_eq!(state.jwt.as_deref(), Some("new-jwt"));
        assert_eq!(state.cookie, None);
        assert_eq!(state.clerk_client_cookie, None);
        assert_eq!(state.session_id, None);
        assert_ne!(state.device_id.as_deref(), Some("old-device"));
        assert!(state.browser_environment.is_none());
    }

    #[test]
    fn verified_auth_state_uses_client_snapshot_after_refresh() {
        let stale_state = AuthState {
            jwt: Some("old-jwt".into()),
            session_id: Some("session".into()),
            device_id: Some("device".into()),
            clerk_client_cookie: Some("client".into()),
            ..Default::default()
        };
        let client =
            SunoClient::new_for_tests("http://127.0.0.1".into(), stale_state).expect("client");
        {
            let mut auth = client.auth.lock().expect("auth mutex");
            auth.jwt = Some("new-jwt".into());
        }

        let saved = verified_auth_state(&client);

        assert_eq!(saved.jwt.as_deref(), Some("new-jwt"));
        assert_eq!(saved.device_id.as_deref(), Some("device"));
    }

    #[test]
    fn logout_removes_stored_auth_and_interactive_browser_profile() {
        let mut deleted_auth = false;
        let mut deleted_profile = false;
        let mut deleted_captcha_profile = false;

        run_logout_with_cleanup(
            || {
                deleted_auth = true;
                Ok(())
            },
            || {
                deleted_profile = true;
                Ok(())
            },
            || {
                deleted_captcha_profile = true;
                Ok(())
            },
        )
        .expect("logout");

        assert!(deleted_auth);
        assert!(deleted_profile);
        assert!(deleted_captcha_profile);
    }
}
