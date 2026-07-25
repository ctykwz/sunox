use serde_json::json;

use super::SunoClient;
use super::types::CowriteLyricsResponse;
use crate::core::CliError;

impl SunoClient {
    /// Generate fresh lyrics through Suno's current Cowrite endpoint.
    pub async fn generate_lyrics(&self, prompt: &str) -> Result<CowriteLyricsResponse, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/generate/cowrite-lyrics/")
                .json(&json!({
                    "selected": "",
                    "context_before": "",
                    "context_after": "",
                    "instruction": prompt,
                    "title": "",
                    "style": "",
                    "mode": "apply_user_request",
                    "references": [],
                    "num_variants": null,
                    "lyricist_id": null,
                    "metadata": {
                        "lyrics_model": "default",
                        "enable_thinking": false
                    },
                    "create_session_token": null,
                    "lyrics_project_id": null
                }))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}
