use super::SunoClient;
use super::types::{Clip, GenerateRequest, GenerateResponse};
use crate::core::CliError;

const FEED_IDS_CHUNK_SIZE: usize = 2;

impl SunoClient {
    /// Submit a music generation request (custom mode or inspiration mode).
    /// Posts only to the current `/api/generate/v2-web/` route. The older
    /// `/api/generate/v2/` route returned `Token validation failed` after Suno
    /// migrated creates to `v2-web` server-side in the April 2026 capture.
    /// Wrapped in `with_auth_retry` so a single stale-JWT failure recovers
    /// transparently via Clerk refresh.
    pub async fn generate(&self, req: &GenerateRequest) -> Result<Vec<Clip>, CliError> {
        if req.token.is_none() {
            let mut challenge = self.generation_challenge().await?;
            if challenge.required && self.try_refresh_jwt_for_challenge_recheck().await? {
                challenge = self.generation_challenge().await?;
            }
            if challenge.required {
                return Err(generation_challenge_error(&challenge));
            }
        }

        let body = self.generation_request_body(req).await?;
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/generate/v2-web/")
                .json(&body)
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let result: GenerateResponse = resp.json().await?;
            Ok(result.clips)
        })
        .await
    }

    /// Fetch clips by IDs. Batches in pairs because Suno's feed endpoint can
    /// drop results when queried with larger mixed batches.
    /// Each chunk is wrapped in `with_auth_retry` so explicit wait/status
    /// flows survive Suno's JWT staleness window mid-generation.
    pub async fn get_clips(&self, ids: &[String]) -> Result<Vec<Clip>, CliError> {
        let mut all_clips = Vec::new();
        for chunk in ids.chunks(FEED_IDS_CHUNK_SIZE) {
            let ids_param = chunk.join(",");
            let path = format!("/api/feed/?ids={ids_param}");
            let clips: Vec<Clip> = self
                .with_auth_retry(|| async {
                    let resp = self.get(&path).send().await?;
                    let resp = self.check_response(resp).await?;
                    let clips: Vec<Clip> = resp.json().await?;
                    Ok(clips)
                })
                .await?;
            all_clips.extend(clips);
        }
        Ok(all_clips)
    }

    async fn generation_request_body(
        &self,
        req: &GenerateRequest,
    ) -> Result<serde_json::Value, CliError> {
        let mut body = serde_json::to_value(req)?;
        if generation_user_tier_is_empty(&body)
            && let Some(user_tier) = self.current_generation_user_tier().await
            && let Some(metadata) = body
                .get_mut("metadata")
                .and_then(|value| value.as_object_mut())
        {
            metadata.insert("user_tier".into(), serde_json::Value::String(user_tier));
        }
        Ok(body)
    }

    async fn current_generation_user_tier(&self) -> Option<String> {
        self.billing_info()
            .await
            .ok()
            .and_then(|info| info.plan.id)
            .map(|tier| tier.trim().to_string())
            .filter(|tier| !tier.is_empty())
    }
}

fn generation_user_tier_is_empty(body: &serde_json::Value) -> bool {
    body.get("metadata")
        .and_then(|metadata| metadata.get("user_tier"))
        .and_then(|user_tier| user_tier.as_str())
        .map(|user_tier| user_tier.trim().is_empty())
        .unwrap_or(true)
}

fn generation_challenge_error(challenge: &super::challenge::GenerationChallenge) -> CliError {
    let version = challenge
        .captcha_version
        .map(|version| version.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    CliError::Config(format!(
        "Suno requires a generation challenge (captcha_version={version}). When stored Clerk refresh material is available, Sunox refreshes the JWT once and repeats the challenge preflight before showing this message. Complete a manual generation challenge in the Suno web app and retry, provide a valid challenge token with --token <token>, or force the browser-backed solver with --captcha."
    ))
}

#[cfg(test)]
mod tests {
    use super::FEED_IDS_CHUNK_SIZE;

    #[test]
    fn feed_id_batch_size_documents_current_web_limit() {
        assert_eq!(FEED_IDS_CHUNK_SIZE, 2);
    }
}
