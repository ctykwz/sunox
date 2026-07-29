use super::SunoClient;
use super::types::{Clip, ConcatRequest, GenerationResult};
use crate::core::CliError;

impl SunoClient {
    pub async fn concat(&self, clip_id: &str) -> Result<GenerationResult, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/generate/concat/v2/")
                .json(&ConcatRequest {
                    clip_id: clip_id.to_string(),
                    is_infill: false,
                })
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let raw: serde_json::Value = resp.json().await?;
            let clip: Clip = serde_json::from_value(raw.clone())?;
            Ok(GenerationResult::from_clip(clip, raw))
        })
        .await
    }
}
