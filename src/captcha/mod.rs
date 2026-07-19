//! Generation challenge solving via a piloted Chromium-family browser instance.

mod browser;
mod cdp;
mod cookies;

use crate::api::challenge::ChallengeProvider;
use crate::auth::AuthState;
use crate::core::CliError;

pub(super) const SUNO_HCAPTCHA_SITEKEY: &str = "d65453de-3f1a-4aac-9366-a0f06e52b2ce";
pub(super) const SUNO_TURNSTILE_SITEKEY: &str = "0x4AAAAAADI7xDNyj-3LcIbi";
pub(super) const CDP_HOST: &str = "127.0.0.1";

/// Solve a fresh browser challenge and return the token to attach to a
/// `/api/generate/v2-web/` request body.
pub async fn solve(auth: &AuthState, provider: ChallengeProvider) -> Result<String, CliError> {
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

pub(crate) fn delete_legacy_browser_profile() -> Result<(), CliError> {
    browser::delete_legacy_profile()
}
