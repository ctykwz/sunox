use std::time::Duration;

use tokio::time::sleep;

use super::session::CdpSession;
use crate::api::challenge::ChallengeProvider;
use crate::auth::AuthState;
use crate::captcha::cookies::extract_cookies;
use crate::captcha::{
    SUNO_HCAPTCHA_ASSET_HOST, SUNO_HCAPTCHA_ENDPOINT, SUNO_HCAPTCHA_IMAGE_HOST,
    SUNO_HCAPTCHA_REPORT_API, SUNO_HCAPTCHA_SCRIPT_URL, SUNO_HCAPTCHA_SITEKEY,
    SUNO_TURNSTILE_SCRIPT_URL,
};
use crate::core::CliError;

// Bound one recoverable error reset in both idle and interactive states.
const SOLVER_EVALUATION_TIMEOUT_MS: u64 = 300_000;
const SUNO_PAGE_DISCOVERY_URL: &str = "https://suno.com/";
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
            serde_json::json!({ "url": SUNO_PAGE_DISCOVERY_URL }),
        )
        .await?;

    let managed_page_url = wait_for_suno_page(session).await?;
    wait_for_provider(session, provider, &managed_page_url).await?;
    sleep(Duration::from_secs(2)).await;
    ensure_managed_suno_page(session, &managed_page_url).await?;

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
    ensure_managed_suno_page(session, &managed_page_url).await?;

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

async fn current_clean_suno_page(session: &mut CdpSession) -> Result<Option<String>, CliError> {
    let probe = session
        .call(
            "Runtime.evaluate",
            serde_json::json!({
                "expression": "document.readyState !== 'loading' && !!document.head && !!document.body ? location.href : ''",
                "returnByValue": true,
            }),
        )
        .await?;
    let href = probe
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    Ok(clean_suno_page_url(href))
}

fn clean_suno_page_url(value: &str) -> Option<String> {
    if value.len() > 2_048 {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("suno.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    Some(url.to_string())
}

fn observe_stable_suno_page(
    previous: &mut Option<String>,
    current: Option<String>,
) -> Option<String> {
    match current {
        Some(current) if previous.as_ref() == Some(&current) => Some(current),
        Some(current) => {
            *previous = Some(current);
            None
        }
        None => {
            *previous = None;
            None
        }
    }
}

async fn wait_for_suno_page(session: &mut CdpSession) -> Result<String, CliError> {
    let mut previous = None;
    for _ in 0..30 {
        sleep(Duration::from_millis(500)).await;
        if let Some(page_url) =
            observe_stable_suno_page(&mut previous, current_clean_suno_page(session).await?)
        {
            return Ok(page_url);
        }
    }

    let page_state = page_state_excerpt(session).await?;
    Err(CliError::Config(format!(
        "The resolved Suno page never became ready ({page_state})"
    )))
}

async fn ensure_managed_suno_page(
    session: &mut CdpSession,
    expected_url: &str,
) -> Result<(), CliError> {
    if current_clean_suno_page(session).await?.as_deref() == Some(expected_url) {
        return Ok(());
    }
    let page_state = page_state_excerpt(session).await?;
    Err(CliError::Config(format!(
        "The resolved Suno page changed during challenge execution ({page_state})"
    )))
}

async fn wait_for_provider(
    session: &mut CdpSession,
    provider: ChallengeProvider,
    managed_page_url: &str,
) -> Result<(), CliError> {
    ensure_managed_suno_page(session, managed_page_url).await?;
    load_provider_script(session, provider).await?;

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
            ensure_managed_suno_page(session, managed_page_url).await?;
            return Ok(());
        }
    }

    let page_state = page_state_excerpt(session).await?;
    Err(CliError::Config(format!(
        "{} never finished loading on the resolved Suno page ({page_state})",
        provider.label()
    )))
}

async fn load_provider_script(
    session: &mut CdpSession,
    provider: ChallengeProvider,
) -> Result<(), CliError> {
    session
        .call(
            "Runtime.evaluate",
            serde_json::json!({ "expression": provider_loader_script(provider) }),
        )
        .await?;
    Ok(())
}

fn provider_loader_script(provider: ChallengeProvider) -> String {
    let (global_name, dataset_property, data_attribute, script_url) = match provider {
        ChallengeProvider::HCaptcha => (
            "hcaptcha",
            "sunoxHcaptcha",
            "sunox-hcaptcha",
            SUNO_HCAPTCHA_SCRIPT_URL,
        ),
        ChallengeProvider::Turnstile => (
            "turnstile",
            "sunoxTurnstile",
            "sunox-turnstile",
            SUNO_TURNSTILE_SCRIPT_URL,
        ),
    };
    format!(
        r#"
            (() => {{
                if (window.{global_name} || document.querySelector('script[data-{data_attribute}]')) {{
                    return;
                }}
                const script = document.createElement('script');
                script.src = '{script_url}';
                script.async = true;
                script.defer = true;
                script.dataset.{dataset_property} = 'true';
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
                "expression": "JSON.stringify({ href: location.href, readyState: document.readyState, hasHead: !!document.head, hasBody: !!document.body })",
                "returnByValue": true,
            }),
        )
        .await?;
    let raw = state
        .get("result")
        .and_then(|result| result.get("value"))
        .and_then(|value| value.as_str())
        .unwrap_or("{}");
    Ok(sanitized_page_state(raw))
}

fn sanitized_page_state(raw: &str) -> String {
    let parsed: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
    let href = parsed
        .get("href")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let page = sanitized_suno_page_label(href);
    let ready_state = parsed
        .get("readyState")
        .and_then(|value| value.as_str())
        .filter(|value| matches!(*value, "loading" | "interactive" | "complete"))
        .unwrap_or("unknown");
    let has_head = parsed
        .get("hasHead")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let has_body = parsed
        .get("hasBody")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    format!("page={page}; ready_state={ready_state}; head={has_head}; body={has_body}")
}

fn sanitized_suno_page_label(value: &str) -> String {
    if value.len() > 131_072 {
        return "untrusted_page".into();
    }
    let Ok(mut url) = reqwest::Url::parse(value) else {
        return "untrusted_page".into();
    };
    if url.scheme() != "https"
        || url.host_str() != Some("suno.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return "untrusted_page".into();
    }
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        SUNO_PAGE_DISCOVERY_URL, clean_suno_page_url, observe_stable_suno_page,
        provider_loader_script, sanitized_page_state, solve_script,
    };
    use crate::api::challenge::ChallengeProvider;
    use crate::captcha::{
        SUNO_HCAPTCHA_ASSET_HOST, SUNO_HCAPTCHA_ENDPOINT, SUNO_HCAPTCHA_IMAGE_HOST,
        SUNO_HCAPTCHA_REPORT_API, SUNO_HCAPTCHA_SCRIPT_URL, SUNO_HCAPTCHA_SITEKEY,
        SUNO_TURNSTILE_IDLE_TIMEOUT_MS, SUNO_TURNSTILE_INTERACTIVE_TIMEOUT_MS,
        SUNO_TURNSTILE_SCRIPT_URL, SUNO_TURNSTILE_SITEKEY,
    };

    #[test]
    fn solver_script_matches_challenge_provider() {
        assert_eq!(SUNO_PAGE_DISCOVERY_URL, "https://suno.com/");

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
        assert!(
            provider_loader_script(ChallengeProvider::HCaptcha).contains(SUNO_HCAPTCHA_SCRIPT_URL)
        );

        let turnstile = solve_script(ChallengeProvider::Turnstile);
        assert!(turnstile.contains("turnstile.render"));
        assert!(turnstile.contains(SUNO_TURNSTILE_SITEKEY));
        assert!(
            provider_loader_script(ChallengeProvider::Turnstile)
                .contains(SUNO_TURNSTILE_SCRIPT_URL)
        );
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

    #[test]
    fn isolated_solver_accepts_only_a_stable_clean_suno_page() {
        for accepted in [
            "https://suno.com/",
            "https://suno.com/create/v3",
            "https://suno.com:443/home/advanced",
        ] {
            assert!(
                clean_suno_page_url(accepted).is_some(),
                "expected clean Suno URL: {accepted}"
            );
        }
        for rejected in [
            "http://suno.com/create",
            "https://www.suno.com/create",
            "https://suno.com:8443/create",
            "https://user:password@suno.com/create",
            "https://suno.com/create?__clerk_handshake=opaque",
            "https://suno.com/create#challenge",
        ] {
            assert!(
                clean_suno_page_url(rejected).is_none(),
                "expected unsafe Suno URL to fail closed: {rejected}"
            );
        }

        let first = clean_suno_page_url("https://suno.com/create/v3");
        let second = clean_suno_page_url("https://suno.com/library");
        let mut previous = None;
        assert_eq!(observe_stable_suno_page(&mut previous, first.clone()), None);
        assert_eq!(
            observe_stable_suno_page(&mut previous, second.clone()),
            None
        );
        assert_eq!(
            observe_stable_suno_page(&mut previous, second.clone()),
            second
        );
        assert_eq!(observe_stable_suno_page(&mut previous, None), None);
        assert_eq!(previous, None);
    }

    #[test]
    fn isolated_solver_diagnostics_do_not_expose_page_or_clerk_secrets() {
        let diagnostic = sanitized_page_state(
            r#"{
                "href":"https://suno.com/create?__clerk_handshake=clerk-secret#fragment-secret",
                "readyState":"interactive",
                "hasHead":true,
                "hasBody":true,
                "body":"private account content"
            }"#,
        );
        assert_eq!(
            diagnostic,
            "page=https://suno.com/create; ready_state=interactive; head=true; body=true"
        );
        for secret in [
            "__clerk_handshake",
            "clerk-secret",
            "fragment-secret",
            "private account content",
        ] {
            assert!(!diagnostic.contains(secret));
        }

        let untrusted = sanitized_page_state(
            r#"{
                "href":"https://user:password@attacker.example/secret?token=value",
                "readyState":"forged",
                "hasHead":true,
                "hasBody":false
            }"#,
        );
        assert_eq!(
            untrusted,
            "page=untrusted_page; ready_state=unknown; head=true; body=false"
        );
        assert!(!untrusted.contains("password"));
        assert!(!untrusted.contains("token"));
    }
}
