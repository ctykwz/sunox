use super::SunoClient;
use super::mutation::MutationSpec;
use super::types::{Clip, ConcatRequest, GenerationResult};
use crate::core::CliError;

impl SunoClient {
    pub async fn concat(&self, clip_id: &str) -> Result<GenerationResult, CliError> {
        let spec = MutationSpec::new(
            "clip_concat",
            format!("clip {clip_id}"),
            vec!["sunox clip list --json".into()],
        )
        .with_context("clip_id", serde_json::Value::String(clip_id.to_string()));
        let raw: serde_json::Value = self
            .mutation_json_once(
                self.post_without_redirect("/api/generate/concat/v2/")
                    .json(&ConcatRequest {
                        clip_id: clip_id.to_string(),
                        is_infill: false,
                    }),
                &spec,
            )
            .await?;
        let clip: Clip = serde_json::from_value(raw.clone()).map_err(|error| {
            spec.ambiguous("response_schema", "schema_drift", error.to_string())
        })?;
        if clip.id.trim().is_empty() {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                "concat returned a blank clip id".into(),
            ));
        }
        Ok(GenerationResult::from_clip(clip, raw))
    }
}
