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
#[cfg(test)]
pub(super) const SUNO_TURNSTILE_SITEKEY: &str = "0x4AAAAAADI7xDNyj-3LcIbi";
pub(super) const SUNO_TURNSTILE_SCRIPT_URL: &str =
    "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
#[cfg(test)]
pub(super) const SUNO_CHALLENGE_SDK_READY_TIMEOUT_MS: u64 = 15_000;
#[cfg(test)]
pub(super) const SUNO_TURNSTILE_IDLE_TIMEOUT_MS: u64 = 15_000;
#[cfg(test)]
pub(super) const SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS: u64 = 120_000;
pub(super) const SUNO_HCAPTCHA_ENDPOINT: &str = "https://hcaptcha-endpoint-prod.suno.com";
pub(super) const SUNO_HCAPTCHA_ASSET_HOST: &str = "https://hcaptcha-assets-prod.suno.com";
pub(super) const SUNO_HCAPTCHA_IMAGE_HOST: &str = "https://hcaptcha-imgs-prod.suno.com";
pub(super) const SUNO_HCAPTCHA_REPORT_API: &str = "https://hcaptcha-reportapi-prod.suno.com";
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
                    "the configured Sunox Browser Bridge did not respond; run `sunox install-browser-extension --force`, then click Reload on the extension in Chrome and keep it enabled. Use `-c challenge_browser=isolated` only when a separate challenge browser is acceptable"
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
    use super::{
        SUNO_CHALLENGE_SDK_READY_TIMEOUT_MS, SUNO_HCAPTCHA_ASSET_HOST, SUNO_HCAPTCHA_ENDPOINT,
        SUNO_HCAPTCHA_IMAGE_HOST, SUNO_HCAPTCHA_REPORT_API, SUNO_HCAPTCHA_SITEKEY,
        SUNO_TURNSTILE_IDLE_TIMEOUT_MS, SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS,
        SUNO_TURNSTILE_SCRIPT_URL, SUNO_TURNSTILE_SITEKEY, bridge_failure_is_terminal,
    };
    use crate::api::challenge::ChallengeProvider;
    use crate::core::ChallengeBrowserMode;

    fn assert_bridge_protocol_constant(page: &str, name: &str, expected: &str) {
        let declaration = format!("const {name} = {expected:?};");
        assert!(
            page.contains(&declaration),
            "Browser Bridge page.js must declare {declaration}"
        );
    }

    fn assert_bridge_protocol_number(page: &str, name: &str, expected: u64) {
        let declaration = format!("const {name} = {expected};");
        assert!(
            page.contains(&declaration),
            "Browser Bridge page.js must declare {declaration}"
        );
    }

    #[test]
    fn browser_bridge_protocol_matches_isolated_solver() {
        let page = include_str!("../../assets/browser-extension/page.js");
        for (name, expected) in [
            (
                "HCAPTCHA_PROVIDER",
                ChallengeProvider::HCaptcha.bridge_name(),
            ),
            (
                "TURNSTILE_PROVIDER",
                ChallengeProvider::Turnstile.bridge_name(),
            ),
            ("HCAPTCHA_SITEKEY", SUNO_HCAPTCHA_SITEKEY),
            ("TURNSTILE_SITEKEY", SUNO_TURNSTILE_SITEKEY),
            ("TURNSTILE_SCRIPT", SUNO_TURNSTILE_SCRIPT_URL),
            ("HCAPTCHA_ENDPOINT", SUNO_HCAPTCHA_ENDPOINT),
            ("HCAPTCHA_ASSET_HOST", SUNO_HCAPTCHA_ASSET_HOST),
            ("HCAPTCHA_IMAGE_HOST", SUNO_HCAPTCHA_IMAGE_HOST),
            ("HCAPTCHA_REPORT_API", SUNO_HCAPTCHA_REPORT_API),
        ] {
            assert_bridge_protocol_constant(page, name, expected);
        }
        assert_bridge_protocol_number(
            page,
            "CHALLENGE_SDK_READY_TIMEOUT_MS",
            SUNO_CHALLENGE_SDK_READY_TIMEOUT_MS,
        );
        assert_bridge_protocol_number(
            page,
            "TURNSTILE_IDLE_TIMEOUT_MS",
            SUNO_TURNSTILE_IDLE_TIMEOUT_MS,
        );
        assert_bridge_protocol_number(
            page,
            "TURNSTILE_INTERACTIVE_TIMEOUT_MS",
            SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS,
        );
    }

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
