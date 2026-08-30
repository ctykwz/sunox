use super::SunoClient;
use super::mutation::MutationSpec;
use super::types::{PromptUpsampleRequest, PromptUpsampleResponse};
use crate::core::CliError;

#[derive(serde::Deserialize)]
struct PersonalizationSettings {
    #[serde(default)]
    styles_augmentation: Option<bool>,
}

impl SunoClient {
    /// Read the account setting used by current Web Create when serializing
    /// metadata.last_tags_generation.personalization_enabled.
    pub async fn styles_augmentation_enabled(&self) -> Result<bool, CliError> {
        self.with_auth_retry(|| async {
            let settings: PersonalizationSettings = self
                .read_json_with_transport_retry(self.get("/api/personalization/settings"))
                .await?;
            Ok(settings.styles_augmentation.unwrap_or(true))
        })
        .await
    }

    /// Ask Suno to enhance style tags. When used before generation, the web
    /// client carries the returned request_id into metadata.last_tags_generation.
    pub async fn upsample_tags(
        &self,
        req: PromptUpsampleRequest<'_>,
    ) -> Result<PromptUpsampleResponse, CliError> {
        let spec = MutationSpec::new("prompt_upsample", "style tags", Vec::new());
        let response: PromptUpsampleResponse = self
            .mutation_json_once(
                self.post_without_redirect("/api/prompts/upsample")
                    .json(&req),
                &spec,
            )
            .await?;
        if response.request_id.trim().is_empty() || response.upsampled.trim().is_empty() {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                "prompt upsample returned a blank request id or result".into(),
            ));
        }
        Ok(response)
    }
}
