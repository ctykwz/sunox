use super::SunoClient;
use super::types::{Clip, GenerateResponse};
use crate::core::CliError;
use serde::Serialize;

#[derive(Serialize)]
struct RemasterRequest<'a> {
    clip_id: &'a str,
    model_name: &'a str,
    variation_category: &'static str,
}

impl SunoClient {
    /// Remaster a clip with a different model version.
    /// Posts to the current web remaster route captured as `/api/generate/upsample`.
    pub async fn remaster(
        &self,
        clip_id: &str,
        remaster_model_key: &str,
    ) -> Result<Vec<Clip>, CliError> {
        let req = RemasterRequest {
            clip_id,
            model_name: remaster_model_key,
            variation_category: "normal",
        };
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/generate/upsample")
                .json(&req)
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let result: GenerateResponse = resp.json().await?;
            result.into_clips()
        })
        .await
    }
}
