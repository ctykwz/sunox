//! Generation challenge solving via a piloted Chromium-family browser instance.

pub(crate) mod bridge_contract;
mod browser;
mod cdp;
mod cookies;
mod existing;

use crate::api::challenge::ChallengeProvider;
use crate::auth::AuthState;
use crate::core::{ChallengeBrowserMode, CliError};

pub(super) const SUNO_HCAPTCHA_SITEKEY: &str = "d65453de-3f1a-4aac-9366-a0f06e52b2ce";
pub(super) const SUNO_TURNSTILE_SITEKEY: &str = "0x4AAAAAADI7xDNyj-3LcIbi";
pub(super) const CDP_HOST: &str = "127.0.0.1";

/// Solve a fresh browser challenge and return the token to attach to a
/// `/api/generate/v2-web/` request body.
pub async fn solve(
    auth: &AuthState,
    provider: ChallengeProvider,
    mode: ChallengeBrowserMode,
) -> Result<String, CliError> {
    if mode != ChallengeBrowserMode::Isolated {
        let bridge_configured = existing::is_configured()?;
        match existing::try_solve(provider).await {
            Ok(Some(token)) => return Ok(token),
            Ok(None) if bridge_failure_is_terminal(mode, bridge_configured) => {
                return Err(CliError::Config(
                    "the configured Sunox Browser Bridge did not respond; load or reload it in Chrome and keep the extension enabled. Use `-c challenge_browser=isolated` only when a separate challenge browser is acceptable"
                        .into(),
                ));
            }
            Ok(None) => {}
            Err(error) if bridge_failure_is_terminal(mode, bridge_configured) => return Err(error),
            Err(error) => eprintln!(
                "Warning: silent Browser Bridge verification was unavailable ({error}); falling back to an isolated browser"
            ),
        }
    }

    let browser = browser::launch(auth.browser_environment.as_ref()).await?;
    let result = async {
        let target = cdp::find_or_create_suno_tab(browser.port()).await?;
        cdp::render_and_execute(&target.web_socket_debugger_url, auth, provider).await
    }
    .await;
    let cleanup = browser.shutdown().await;
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(token), Ok(())) => Ok(token),
    }
}

fn bridge_failure_is_terminal(mode: ChallengeBrowserMode, bridge_configured: bool) -> bool {
    mode == ChallengeBrowserMode::Existing
        || (mode == ChallengeBrowserMode::Auto && bridge_configured)
}

pub(crate) fn delete_legacy_browser_profile() -> Result<(), CliError> {
    browser::delete_legacy_profile()
}

#[cfg(test)]
mod tests {
    use super::bridge_failure_is_terminal;
    use crate::core::ChallengeBrowserMode;

    #[test]
    fn configured_bridge_fails_closed_in_auto_mode() {
        assert!(bridge_failure_is_terminal(ChallengeBrowserMode::Auto, true));
        assert!(!bridge_failure_is_terminal(
            ChallengeBrowserMode::Auto,
            false
        ));
    }

    #[test]
    fn explicit_modes_keep_their_strict_browser_policy() {
        assert!(bridge_failure_is_terminal(
            ChallengeBrowserMode::Existing,
            false
        ));
        assert!(!bridge_failure_is_terminal(
            ChallengeBrowserMode::Isolated,
            true
        ));
    }
}
