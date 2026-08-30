use serde::Serialize;

use super::SunoClient;
use super::mutation::MutationSpec;
use super::types::Clip;
use crate::core::CliError;

#[derive(Serialize)]
struct SpeedAdjustRequest<'a> {
    clip_id: &'a str,
    speed_multiplier: f64,
    keep_pitch: bool,
    title: &'a str,
}

impl SunoClient {
    /// Adjust clip playback speed through the current web edit route.
    pub async fn adjust_speed(
        &self,
        clip_id: &str,
        speed_multiplier: f64,
        keep_pitch: bool,
        title: &str,
    ) -> Result<Clip, CliError> {
        let req = SpeedAdjustRequest {
            clip_id,
            speed_multiplier,
            keep_pitch,
            title,
        };
        let spec = MutationSpec::new(
            "clip_adjust_speed",
            format!("clip {clip_id}"),
            vec!["sunox clip list --json".into()],
        )
        .with_context("clip_id", serde_json::Value::String(clip_id.to_string()));
        let clip: Clip = self
            .mutation_json_once(
                self.post_without_redirect("/api/clips/adjust-speed/")
                    .json(&req),
                &spec,
            )
            .await?;
        if clip.id.trim().is_empty() {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                "speed adjustment returned a blank clip id".into(),
            ));
        }
        Ok(clip)
    }
}
