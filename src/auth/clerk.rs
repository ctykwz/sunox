use crate::core::CliError;

const CLERK_BASE: &str = "https://auth.suno.com";
const CLERK_JS_VERSION: &str = "5.117.0";
const CLERK_API_VERSION: &str = "2025-11-10";

fn clerk_client_url() -> String {
    format!(
        "{CLERK_BASE}/v1/client?__clerk_api_version={CLERK_API_VERSION}&_clerk_js_version={CLERK_JS_VERSION}"
    )
}

fn clerk_token_url(session_id: &str) -> String {
    format!(
        "{CLERK_BASE}/v1/client/sessions/{session_id}/tokens?__clerk_api_version={CLERK_API_VERSION}&_clerk_js_version={CLERK_JS_VERSION}"
    )
}

fn apply_clerk_headers(
    builder: reqwest::RequestBuilder,
    clerk_cookie: &str,
) -> reqwest::RequestBuilder {
    builder
        .header("authorization", clerk_cookie)
        .header("cookie", format!("__client={clerk_cookie}"))
        .header("origin", "https://suno.com")
        .header("referer", "https://suno.com/")
}

fn response_excerpt(body: &str) -> String {
    const MAX: usize = 500;
    let body = body.replace(['\n', '\r'], " ");
    if body.len() <= MAX {
        body
    } else {
        format!("{}...", body.chars().take(MAX).collect::<String>())
    }
}

fn clerk_status_code(
    status: reqwest::StatusCode,
    rejected: &'static str,
    failed: &'static str,
) -> &'static str {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        "clerk_rate_limited"
    } else if status.is_client_error()
        && status != reqwest::StatusCode::REQUEST_TIMEOUT
        && status != reqwest::StatusCode::TOO_EARLY
    {
        rejected
    } else {
        failed
    }
}

/// Exchange the __client cookie for a session ID and JWT via Clerk.
pub async fn clerk_token_exchange(
    client: &reqwest::Client,
    clerk_cookie: &str,
) -> Result<(String, String), CliError> {
    let resp = apply_clerk_headers(client.get(clerk_client_url()), clerk_cookie)
        .send()
        .await
        .map_err(CliError::Http)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CliError::Api {
            code: clerk_status_code(status, "clerk_exchange_rejected", "clerk_exchange_failed"),
            message: format!(
                "Clerk token exchange failed ({status}): {}",
                response_excerpt(&body)
            ),
        });
    }

    let body: serde_json::Value = resp.json().await.map_err(CliError::Http)?;
    let session_id = body
        .get("response")
        .and_then(|r| {
            r.get("last_active_session_id")
                .and_then(|s| s.as_str())
                .or_else(|| {
                    r.get("sessions")
                        .and_then(|s| s.as_array())
                        .and_then(|sessions| sessions.first())
                        .and_then(|session| session.get("id"))
                        .and_then(|id| id.as_str())
                })
        })
        .ok_or_else(|| CliError::Api {
            code: "no_session",
            message: "No active session found - log into suno.com in your browser first".into(),
        })?
        .to_string();

    let jwt = clerk_refresh_jwt(client, clerk_cookie, &session_id).await?;
    Ok((session_id, jwt))
}

/// Refresh JWT using stored Clerk cookie + session ID.
pub async fn clerk_refresh_jwt(
    client: &reqwest::Client,
    clerk_cookie: &str,
    session_id: &str,
) -> Result<String, CliError> {
    let resp = apply_clerk_headers(client.post(clerk_token_url(session_id)), clerk_cookie)
        .header("content-type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(CliError::Http)?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(CliError::Api {
            code: clerk_status_code(status, "clerk_refresh_rejected", "clerk_refresh_failed"),
            message: format!(
                "Clerk JWT refresh failed ({status}): {}",
                response_excerpt(&body)
            ),
        });
    }

    let body: serde_json::Value = resp.json().await.map_err(CliError::Http)?;
    body.get("jwt")
        .and_then(|j| j.as_str())
        .map(String::from)
        .ok_or_else(|| CliError::Api {
            code: "no_jwt",
            message: "Clerk returned no JWT - session may have expired, run `sunox login` again"
                .into(),
        })
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::clerk_status_code;

    #[test]
    fn clerk_status_distinguishes_rejection_from_server_failure() {
        assert_eq!(
            clerk_status_code(StatusCode::UNAUTHORIZED, "rejected", "failed"),
            "rejected"
        );
        assert_eq!(
            clerk_status_code(StatusCode::SERVICE_UNAVAILABLE, "rejected", "failed"),
            "failed"
        );
        assert_eq!(
            clerk_status_code(StatusCode::TOO_MANY_REQUESTS, "rejected", "failed"),
            "clerk_rate_limited"
        );
        assert_eq!(
            clerk_status_code(StatusCode::REQUEST_TIMEOUT, "rejected", "failed"),
            "failed"
        );
    }
}
