use std::time::Duration;

use tokio::time::sleep;

use super::session::CdpSession;
use crate::api::challenge::ChallengeProvider;
use crate::auth::AuthState;
use crate::captcha::cookies::extract_cookies;
use crate::captcha::{
    SUNO_HCAPTCHA_ASSET_HOST, SUNO_HCAPTCHA_ENDPOINT, SUNO_HCAPTCHA_IMAGE_HOST,
    SUNO_HCAPTCHA_REPORT_API, SUNO_HCAPTCHA_SITEKEY, SUNO_TURNSTILE_SCRIPT_URL,
};
use crate::core::CliError;

// Bound one recoverable error reset in both idle and interactive states.
const SOLVER_EVALUATION_TIMEOUT_MS: u64 = 300_000;
#[cfg(test)]
const _: () = assert!(
    SOLVER_EVALUATION_TIMEOUT_MS
        > 3 * crate::captcha::SUNO_TURNSTILE_IDLE_TIMEOUT_MS
            + 2 * crate::captcha::SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS
);

pub(in crate::captcha) async fn render_and_execute(
    ws_url: &str,
    auth: &AuthState,
    provider: ChallengeProvider,
) -> Result<String, CliError> {
    let mut session = CdpSession::connect(ws_url).await?;

    let result = execute_with_session(&mut session, auth, provider).await;
    let cleanup = session
        .call("Network.clearBrowserCookies", serde_json::json!({}))
        .await;
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(token), Ok(_)) => Ok(token),
    }
}

async fn execute_with_session(
    session: &mut CdpSession,
    auth: &AuthState,
    provider: ChallengeProvider,
) -> Result<String, CliError> {
    session
        .call("Network.enable", serde_json::json!({}))
        .await?;
    session.call("Page.enable", serde_json::json!({})).await?;
    session
        .call("Runtime.enable", serde_json::json!({}))
        .await?;
    session
        .call(
            "Emulation.setDeviceMetricsOverride",
            serde_json::json!({
                "width": 1280,
                "height": 900,
                "deviceScaleFactor": 1,
                "mobile": false
            }),
        )
        .await?;

    session
        .call("Network.clearBrowserCookies", serde_json::json!({}))
        .await?;

    let cookies = extract_cookies(auth)?;
    if !cookies.is_empty() {
        session
            .call(
                "Network.setCookies",
                serde_json::json!({ "cookies": cookies }),
            )
            .await?;
    }

    session
        .call(
            "Page.navigate",
            serde_json::json!({ "url": "https://suno.com/create" }),
        )
        .await?;

    wait_for_suno_page(session).await?;
    wait_for_provider(session, provider).await?;
    sleep(Duration::from_secs(2)).await;

    let result = session
        .call_with_timeout(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": solve_script(provider),
                "awaitPromise": true,
                "returnByValue": true,
            }),
            Duration::from_millis(SOLVER_EVALUATION_TIMEOUT_MS),
        )
        .await?;

    let token = result
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    if token.is_empty() {
        return Err(CliError::Config(format!(
            "{} returned an empty token",
            provider.label()
        )));
    }
    if token.starts_with("ERR:") {
        return Err(CliError::Config(format!(
            "{} solver: {token}",
            provider.label()
        )));
    }
    Ok(token)
}

async fn wait_for_suno_page(session: &mut CdpSession) -> Result<(), CliError> {
    for _ in 0..30 {
        sleep(Duration::from_millis(500)).await;
        let probe = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "location.hostname === 'suno.com' && document.readyState !== 'loading' && !!document.head && !!document.body",
                    "returnByValue": true,
                }),
            )
            .await?;
        if probe
            .get("result")
            .and_then(|result| result.get("value"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Ok(());
        }
    }

    let page_state = page_state_excerpt(session).await?;
    Err(CliError::Config(format!(
        "Suno create page never became ready ({page_state})"
    )))
}

async fn wait_for_provider(
    session: &mut CdpSession,
    provider: ChallengeProvider,
) -> Result<(), CliError> {
    if provider == ChallengeProvider::Turnstile {
        load_turnstile_script(session).await?;
    }

    let probe_expression = match provider {
        ChallengeProvider::HCaptcha => "typeof hcaptcha !== 'undefined' && !!hcaptcha.render",
        ChallengeProvider::Turnstile => {
            "typeof turnstile !== 'undefined' && !!turnstile.render && !!turnstile.execute"
        }
    };
    for _ in 0..30 {
        sleep(Duration::from_secs(1)).await;
        let probe = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": probe_expression,
                    "returnByValue": true,
                }),
            )
            .await?;
        if probe
            .get("result")
            .and_then(|result| result.get("value"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            return Ok(());
        }
    }

    let page_state = page_state_excerpt(session).await?;
    Err(CliError::Config(format!(
        "{} never finished loading on suno.com/create ({page_state})",
        provider.label()
    )))
}

async fn load_turnstile_script(session: &mut CdpSession) -> Result<(), CliError> {
    session
        .call(
            "Runtime.evaluate",
            serde_json::json!({ "expression": turnstile_loader_script() }),
        )
        .await?;
    Ok(())
}

fn turnstile_loader_script() -> String {
    format!(
        r#"
            (() => {{
                if (window.turnstile || document.querySelector('script[data-sunox-turnstile]')) {{
                    return;
                }}
                const script = document.createElement('script');
                script.src = '{SUNO_TURNSTILE_SCRIPT_URL}';
                script.async = true;
                script.defer = true;
                script.dataset.sunoxTurnstile = 'true';
                document.head.appendChild(script);
            }})()
        "#
    )
}

fn solve_script(provider: ChallengeProvider) -> String {
    match provider {
        ChallengeProvider::HCaptcha => hcaptcha_solve_script(),
        ChallengeProvider::Turnstile => turnstile_solve_script(),
    }
}

fn hcaptcha_solve_script() -> String {
    format!(
        r#"
        (async () => {{
            try {{
                const div = document.createElement('div');
                div.style.cssText = 'position:fixed;top:-9999px;left:-9999px;';
                document.body.appendChild(div);
                const id = hcaptcha.render(div, {{
                    sitekey: '{SUNO_HCAPTCHA_SITEKEY}',
                    size: 'invisible',
                    sentry: false,
                    endpoint: '{SUNO_HCAPTCHA_ENDPOINT}',
                    assethost: '{SUNO_HCAPTCHA_ASSET_HOST}',
                    imghost: '{SUNO_HCAPTCHA_IMAGE_HOST}',
                    reportapi: '{SUNO_HCAPTCHA_REPORT_API}',
                }});
                const r = await hcaptcha.execute(id, {{ async: true }});
                return (r && r.response) ? r.response : '';
            }} catch (e) {{
                return 'ERR:' + String(e);
            }}
        }})()
        "#
    )
}

fn turnstile_solve_script() -> String {
    include_str!("turnstile_solver.js").to_string()
}

async fn page_state_excerpt(session: &mut CdpSession) -> Result<String, CliError> {
    let state = session
        .call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "JSON.stringify({ href: location.href, body: (document.body && document.body.innerText || '').slice(0, 240) })",
                "returnByValue": true,
            }),
        )
        .await?;
    let raw = state
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or("{}");
    let parsed: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
    let href = parsed
        .get("href")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let body = parsed
        .get("body")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .replace(['\n', '\r'], " ");
    if body.is_empty() {
        Ok(format!("page={href}"))
    } else {
        Ok(format!("page={href}; body={body}"))
    }
}

#[cfg(test)]
mod tests {
    use super::{solve_script, turnstile_loader_script};
    use crate::api::challenge::ChallengeProvider;
    use crate::captcha::{
        SUNO_HCAPTCHA_ASSET_HOST, SUNO_HCAPTCHA_ENDPOINT, SUNO_HCAPTCHA_IMAGE_HOST,
        SUNO_HCAPTCHA_REPORT_API, SUNO_HCAPTCHA_SITEKEY, SUNO_TURNSTILE_IDLE_TIMEOUT_MS,
        SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS, SUNO_TURNSTILE_SCRIPT_URL, SUNO_TURNSTILE_SITEKEY,
    };

    #[test]
    fn solver_script_matches_challenge_provider() {
        let hcaptcha = solve_script(ChallengeProvider::HCaptcha);
        assert!(hcaptcha.contains("hcaptcha.render"));
        for expected in [
            SUNO_HCAPTCHA_SITEKEY,
            SUNO_HCAPTCHA_ENDPOINT,
            SUNO_HCAPTCHA_ASSET_HOST,
            SUNO_HCAPTCHA_IMAGE_HOST,
            SUNO_HCAPTCHA_REPORT_API,
        ] {
            assert!(hcaptcha.contains(expected));
        }
        assert!(hcaptcha.contains("size: 'invisible'"));
        assert!(hcaptcha.contains("sentry: false"));
        assert!(hcaptcha.contains("hcaptcha.execute(id, { async: true })"));
        assert!(hcaptcha.contains("r && r.response"));

        let turnstile = solve_script(ChallengeProvider::Turnstile);
        assert!(turnstile.contains("turnstile.render"));
        assert!(turnstile.contains(SUNO_TURNSTILE_SITEKEY));
        assert!(turnstile_loader_script().contains(SUNO_TURNSTILE_SCRIPT_URL));
        assert!(turnstile.contains("execution: \"execute\""));
        assert!(turnstile.contains("appearance: \"interaction-only\""));
        assert!(turnstile.contains("callback: finish"));
        assert!(turnstile.contains("\"error-callback\""));
        assert!(turnstile.contains("\"expired-callback\""));
        assert!(turnstile.contains("\"timeout-callback\""));
        assert!(turnstile.contains("\"unsupported-callback\""));
        assert!(turnstile.contains("\"before-interactive-callback\""));
        assert!(turnstile.contains("\"after-interactive-callback\""));
        assert!(turnstile.contains(&SUNO_TURNSTILE_IDLE_TIMEOUT_MS.to_string()));
        assert!(turnstile.contains(&SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS.to_string()));
        assert!(turnstile.contains("turnstile.execute(widgetId)"));
        assert!(!turnstile.contains("top:-9999px"));
    }
}
