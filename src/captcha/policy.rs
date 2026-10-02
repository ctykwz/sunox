//! Read the web client's server-seeded provider fallback policy without executing JavaScript.
use std::time::Duration;

use serde_json::Value;

use crate::auth::AuthState;
use crate::core::CliError;

const FLAG: &str = "hcaptchaFallbackOnTurnstileTimeoutEnabled";
const MAX_PAGE_BYTES: usize = 5 * 1024 * 1024;

pub(crate) async fn hcaptcha_fallback_allowed(auth: &AuthState) -> Result<bool, CliError> {
    let Some(jwt) = auth.jwt.as_deref().filter(|jwt| !jwt.trim().is_empty()) else {
        return Ok(false);
    };
    let client = crate::net::proxy::apply_to_client_builder(
        reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(crate::net::http::BROWSER_USER_AGENT)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let url = attempt.url();
                if attempt.previous().len() < 4
                    && url.scheme() == "https"
                    && url.host_str() == Some("suno.com")
                    && url.port_or_known_default() == Some(443)
                    && url.username().is_empty()
                    && url.password().is_none()
                {
                    attempt.follow()
                } else {
                    attempt.stop()
                }
            })),
    )?
    .build()?;
    // SSR reads the Clerk session cookie. Never send the long-lived Clerk
    // __client credential, API bearer header, or cookies to another origin.
    let mut response = client
        .get("https://suno.com/")
        .header(reqwest::header::COOKIE, format!("__session={jwt}"))
        .send()
        .await?
        .error_for_status()?;
    if !response.status().is_success() {
        return Ok(false);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > MAX_PAGE_BYTES {
            return Ok(false);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(fallback_gate_from_html(&String::from_utf8_lossy(&body)))
}

fn fallback_gate_from_html(html: &str) -> bool {
    let mut flight = String::new();
    // Parse only JSON arguments in actual inline Flight bootstrap scripts.
    // Do not match booleans in rendered lyrics, escaped text, or JS source.
    for script in html.split("<script").skip(1) {
        let Some((_, content)) = script.split_once('>') else {
            continue;
        };
        let Some((content, _)) = content.split_once("</script>") else {
            continue;
        };
        let Some(argument) = content.trim().strip_prefix("self.__next_f.push(") else {
            continue;
        };
        let Some(argument) = argument.trim_end_matches(';').strip_suffix(')') else {
            continue;
        };
        let Ok(Value::Array(values)) = serde_json::from_str::<Value>(argument) else {
            continue;
        };
        if values.first().and_then(Value::as_u64) == Some(1)
            && let Some(text) = values.get(1).and_then(Value::as_str)
        {
            flight.push_str(text);
        }
    }
    let mut decisions = Vec::new();
    for row in flight.lines() {
        let Some((_, payload)) = row.split_once(':') else {
            continue;
        };
        if let Ok(value) = serde_json::from_str::<Value>(payload) {
            collect_gate(&value, &mut decisions);
        }
    }
    !decisions.is_empty() && decisions.iter().all(|enabled| *enabled)
}

fn collect_gate(value: &Value, decisions: &mut Vec<bool>) {
    match value {
        Value::Object(object) => {
            if object.contains_key("initialGenerationCaptchaVersion")
                && object.contains_key("eagerTurnstileMountEnabled")
                && let Some(enabled) = object.get(FLAG)
            {
                decisions.push(enabled.as_bool() == Some(true));
            }
            for value in object.values() {
                collect_gate(value, decisions);
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_gate(value, decisions);
            }
        }
        _ => {}
    }
}

/// Only known provider outcomes are retryable; Bridge pairing, transport,
/// document provenance, token validation and cleanup failures are terminal.
pub(super) fn bridge_provider_failed(message: &str) -> bool {
    matches!(
        message,
        "silent_challenge_unavailable: the challenge token expired"
            | "silent_challenge_unavailable: the challenge provider failed"
            | "silent_challenge_unavailable: the challenge SDK did not become ready"
            | "silent_challenge_unavailable: the challenge did not finish before its deadline"
            | "interactive_browser_required: the challenge requires visible browser interaction"
            | "silent_challenge_unavailable: the challenge could not complete silently"
            | "silent_challenge_unavailable: Turnstile error callback (family 100)"
            | "silent_challenge_unavailable: Turnstile error callback (family 110)"
            | "silent_challenge_unavailable: Turnstile error callback (family 200)"
            | "silent_challenge_unavailable: Turnstile error callback (family 300)"
            | "silent_challenge_unavailable: Turnstile error callback (family 400)"
            | "silent_challenge_unavailable: Turnstile error callback (family 600)"
            | "silent_challenge_unavailable: Turnstile reported an unrecognized provider error"
            | "interactive_browser_required: the Turnstile interaction timed out"
            | "silent_challenge_unavailable: Turnstile produced no callback across two fresh widget attempts"
            | "silent_challenge_unavailable: the challenge provider is unsupported in this browser"
    )
}

pub(crate) fn provider_failure() -> CliError {
    CliError::Api {
        code: "challenge_provider_failed",
        message: "The challenge provider did not produce a usable token".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(flag: Value) -> String {
        let row = format!(
            "1:{}\n",
            serde_json::json!(["$", "$L1", null, {
                "initialGenerationCaptchaVersion": 2,
                "eagerTurnstileMountEnabled": true,
                FLAG: flag
            }])
        );
        format!(
            "<script>self.__next_f.push({})</script>",
            serde_json::json!([1, row])
        )
    }

    #[test]
    fn only_explicit_boolean_server_seed_enables_fallback() {
        assert!(fallback_gate_from_html(&seed(Value::Bool(true))));
        for flag in [
            Value::Bool(false),
            Value::Null,
            Value::String("true".into()),
        ] {
            assert!(!fallback_gate_from_html(&seed(flag)));
        }
        assert!(!fallback_gate_from_html(
            "<p>hcaptchaFallbackOnTurnstileTimeoutEnabled:true</p>"
        ));
        assert!(!fallback_gate_from_html(&format!(
            "{}{}",
            seed(Value::Bool(true)),
            seed(Value::Bool(false))
        )));
    }

    #[test]
    fn provider_failures_exclude_bridge_and_security_errors() {
        assert!(bridge_provider_failed(
            "silent_challenge_unavailable: Turnstile produced no callback across two fresh widget attempts"
        ));
        for error in [
            "Managed Suno frame was busy or unavailable",
            "Managed Suno frame returned an invalid challenge token",
            "pairing failed",
            "silent_challenge_unavailable: arbitrary untrusted message",
        ] {
            assert!(!bridge_provider_failed(error));
        }
    }
}
