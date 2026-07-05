use super::SunoClient;
use crate::core::CliError;

impl SunoClient {
    pub async fn check_response(
        &self,
        resp: reqwest::Response,
    ) -> Result<reqwest::Response, CliError> {
        let status = resp.status();
        if status == 401 {
            return Err(CliError::AuthExpired);
        }
        if status == 403 {
            let body = resp.text().await.unwrap_or_default();
            if looks_like_auth_expired(&body) {
                return Err(CliError::AuthExpired);
            }
            return Err(CliError::Api {
                code: "forbidden",
                message: format!("HTTP 403 Forbidden: {body}"),
            });
        }
        if status == 429 {
            return Err(CliError::RateLimited);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            if looks_like_auth_expired(&body) {
                return Err(CliError::AuthExpired);
            }
            if body.contains("'loc': ['body', 'params'")
                || body.contains("\"loc\": [\"body\", \"params\"")
            {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "HTTP {status}: Suno's request schema has changed - the CLI needs an update. Body: {body}"
                    ),
                });
            }
            return Err(CliError::Api {
                code: "api_error",
                message: format!("HTTP {status}: {body}"),
            });
        }
        Ok(resp)
    }
}

fn looks_like_auth_expired(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("token validation failed")
        || lower.contains("jwt expired")
        || lower.contains("jwt is expired")
        || lower.contains("invalid jwt")
        || lower.contains("invalid token")
        || lower.contains("not authenticated")
        || lower.contains("unauthenticated")
}

#[cfg(test)]
mod tests {
    use super::looks_like_auth_expired;

    #[test]
    fn auth_expired_detector_matches_suno_and_clerk_phrases() {
        assert!(looks_like_auth_expired("Token validation failed."));
        assert!(looks_like_auth_expired(r#"{"detail":"JWT expired"}"#));
        assert!(looks_like_auth_expired("not authenticated"));
        assert!(looks_like_auth_expired("invalid token"));
    }

    #[test]
    fn auth_expired_detector_does_not_match_unrelated_failures() {
        assert!(!looks_like_auth_expired("generation challenge required"));
        assert!(!looks_like_auth_expired("unexpected server error"));
    }
}
