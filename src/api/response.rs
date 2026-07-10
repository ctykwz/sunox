use super::SunoClient;
use crate::core::CliError;

impl SunoClient {
    pub async fn check_response(
        &self,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, CliError> {
        self.check_response_with_invalid_token_policy(resp, true)
            .await
    }

    pub(crate) async fn check_generation_response(
        &self,
        resp: reqwest::Response,
        has_challenge_token: bool,
    ) -> Result<reqwest::Response, CliError> {
        self.check_response_with_invalid_token_policy(resp, !has_challenge_token)
            .await
    }

    async fn check_response_with_invalid_token_policy(
        &self,
        resp: reqwest::Response,
        generic_invalid_token_is_auth: bool,
    ) -> Result<reqwest::Response, CliError> {
        let status = resp.status();
        if status == 401 {
            return Err(CliError::AuthExpired);
        }
        if status == 403 {
            let body = resp.text().await.unwrap_or_default();
            if looks_like_auth_expired(&body, generic_invalid_token_is_auth) {
                return Err(CliError::AuthExpired);
            }
            return Err(suno_api_error(status, &body));
        }
        if status == 429 {
            return Err(CliError::RateLimited);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if looks_like_auth_expired(&body, generic_invalid_token_is_auth) {
                return Err(CliError::AuthExpired);
            }
            if body.contains("'loc': ['body', 'params'")
                || body.contains("\"loc\": [\"body\", \"params\"")
            {
                return Err(suno_api_error_with_code("schema_drift", status, &body));
            }
            return Err(suno_api_error(status, &body));
        }
        Ok(resp)
    }
}

fn suno_api_error(status: reqwest::StatusCode, body: &str) -> CliError {
    let code = match status.as_u16() {
        403 => "forbidden",
        404 => "not_found",
        _ => "api_error",
    };
    suno_api_error_with_code(code, status, body)
}

fn suno_api_error_with_code(
    code: &'static str,
    status: reqwest::StatusCode,
    body: &str,
) -> CliError {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let retryable = parsed
        .as_ref()
        .and_then(|value| value.get("retryable"))
        .and_then(serde_json::Value::as_bool);
    let detail = parsed
        .as_ref()
        .and_then(|value| value.get("detail"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            parsed
                .as_ref()
                .and_then(|value| value.get("detail_fallback"))
                .and_then(serde_json::Value::as_str)
        })
        .filter(|detail| !detail.trim().is_empty());
    let message = detail
        .map(|detail| format!("HTTP {status}: {detail}"))
        .unwrap_or_else(|| format!("HTTP {status}: {body}"));
    let mut detail_fields = match parsed {
        Some(serde_json::Value::Object(fields)) => fields,
        Some(response) => serde_json::Map::from_iter([("response".into(), response)]),
        None => {
            let mut fields = serde_json::Map::new();
            if !body.is_empty() {
                fields.insert("body".into(), serde_json::Value::String(body.to_string()));
            }
            fields
        }
    };
    detail_fields.insert("http_status".into(), serde_json::json!(status.as_u16()));
    if let Some(retryable) = retryable {
        detail_fields.insert("retryable".into(), serde_json::json!(retryable));
    }
    let details = Some(serde_json::Value::Object(detail_fields));

    CliError::SunoApi {
        code,
        status: status.as_u16(),
        message,
        retryable,
        details,
    }
}

fn looks_like_auth_expired(body: &str, generic_invalid_token_is_auth: bool) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("token validation failed")
        || lower.contains("jwt expired")
        || lower.contains("jwt is expired")
        || lower.contains("invalid jwt")
        || lower.contains("not authenticated")
        || lower.contains("unauthenticated")
        || (generic_invalid_token_is_auth && lower.contains("invalid token"))
}

#[cfg(test)]
mod tests {
    use super::{looks_like_auth_expired, suno_api_error};
    use crate::core::CliError;

    #[test]
    fn auth_expired_detector_matches_suno_and_clerk_phrases() {
        assert!(looks_like_auth_expired("Token validation failed.", true));
        assert!(looks_like_auth_expired(r#"{"detail":"JWT expired"}"#, true));
        assert!(looks_like_auth_expired("not authenticated", true));
        assert!(looks_like_auth_expired("invalid token", true));
    }

    #[test]
    fn auth_expired_detector_does_not_match_unrelated_failures() {
        assert!(!looks_like_auth_expired(
            "generation challenge required",
            true
        ));
        assert!(!looks_like_auth_expired("invalid token", false));
        assert!(!looks_like_auth_expired("invalid challenge token", false));
        assert!(!looks_like_auth_expired("unexpected server error", true));
    }

    #[test]
    fn structured_suno_error_preserves_retryability_and_details() {
        let error = suno_api_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"status_code":500,"detail":"An unexpected error occurred.","error_type":"server_error","retryable":false}"#,
        );

        match error {
            CliError::SunoApi {
                code,
                status,
                message,
                retryable,
                details,
            } => {
                assert_eq!(code, "api_error");
                assert_eq!(status, 500);
                assert_eq!(retryable, Some(false));
                assert!(message.contains("An unexpected error occurred."));
                let details = details.expect("details");
                assert_eq!(details["error_type"], "server_error");
                assert_eq!(details["http_status"], 500);
                assert_eq!(details["retryable"], false);
            }
            other => panic!("expected structured Suno API error, got {other:?}"),
        }
    }
}
