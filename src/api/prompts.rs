use super::SunoClient;
use super::types::{PromptUpsampleRequest, PromptUpsampleResponse};
use crate::core::CliError;

impl SunoClient {
    /// Ask Suno to enhance style tags. When used before generation, the web
    /// client carries the returned request_id into metadata.last_tags_generation.
    pub async fn upsample_tags(
        &self,
        req: PromptUpsampleRequest<'_>,
    ) -> Result<PromptUpsampleResponse, CliError> {
        self.with_auth_retry(|| async {
            let resp = self.post("/api/prompts/upsample").json(&req).send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}
