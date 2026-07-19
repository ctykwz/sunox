use super::SunoClient;
use super::types::{Clip, ConcatRequest};
use crate::core::CliError;

impl SunoClient {
    pub async fn concat(&self, clip_id: &str) -> Result<Clip, CliError> {
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
            Ok(resp.json().await?)
        })
        .await
    }
}
