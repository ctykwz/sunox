use std::time::Duration;

use tokio::time::sleep;

use super::session::CdpSession;
use crate::auth::AuthState;
use crate::captcha::SUNO_HCAPTCHA_SITEKEY;
use crate::captcha::cookies::extract_cookies;
use crate::core::CliError;

pub(in crate::captcha) async fn render_and_execute(
    ws_url: &str,
    auth: &AuthState,
) -> Result<String, CliError> {
    let mut session = CdpSession::connect(ws_url).await?;

    let result = execute_with_session(&mut session, auth).await;
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

    wait_for_hcaptcha(session).await?;
    sleep(Duration::from_secs(2)).await;

    let result = session
        .call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": solve_script(),
                "awaitPromise": true,
                "returnByValue": true,
            }),
        )
        .await?;

    let token = result
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    if token.is_empty() {
        return Err(CliError::Config("hcaptcha returned empty token".into()));
    }
    if token.starts_with("ERR:") {
        return Err(CliError::Config(format!("hcaptcha solver: {token}")));
    }
    Ok(token)
}

async fn wait_for_hcaptcha(session: &mut CdpSession) -> Result<(), CliError> {
    for _ in 0..30 {
        sleep(Duration::from_secs(1)).await;
        let probe = session
            .call(
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": "typeof hcaptcha !== 'undefined' && !!hcaptcha.render",
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
        "hcaptcha never finished loading on suno.com/create ({page_state})"
    )))
}

fn solve_script() -> String {
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
                    endpoint: 'https://hcaptcha-endpoint-prod.suno.com',
                    assethost: 'https://hcaptcha-assets-prod.suno.com',
                    imghost: 'https://hcaptcha-imgs-prod.suno.com',
                    reportapi: 'https://hcaptcha-reportapi-prod.suno.com',
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
