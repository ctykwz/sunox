use super::SunoClient;
use super::types::{Clip, GenerateRequest, GenerateResponse};
use crate::core::CliError;

impl SunoClient {
    /// Submit a music generation request (custom mode or inspiration mode).
    /// Posts only to the current `/api/generate/v2-web/` route. The older
    /// `/api/generate/v2/` route returned `Token validation failed` after Suno
    /// migrated creates to `v2-web` server-side in the April 2026 capture.
    /// Wrapped in `with_auth_retry` so a single stale-JWT failure recovers
    /// transparently via Clerk refresh.
    pub async fn generate(&self, req: &GenerateRequest) -> Result<Vec<Clip>, CliError> {
        if req.token.is_none() {
            let challenge = self.generation_challenge().await?;
            if challenge.required {
                let version = challenge
                    .captcha_version
                    .map(|version| version.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                return Err(CliError::Config(format!(
                    "Suno requires a generation challenge (captcha_version={version}). Complete a manual generation challenge in the Suno web app and retry, provide a valid challenge token with --token <token>, or force the browser-backed solver with --captcha."
                )));
            }
        }

        self.with_auth_retry(|| async {
            let resp = self.post("/api/generate/v2-web/").json(req).send().await?;
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
        for chunk in ids.chunks(2) {
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
}
