use super::SunoClient;
use super::types::{Clip, GenerateRequest};
use crate::core::CliError;

impl SunoClient {
    /// Extract stems from a clip via the current web `gen_stem` generation task.
    pub async fn stems(
        &self,
        clip_id: &str,
        challenge_token: Option<String>,
    ) -> Result<Vec<Clip>, CliError> {
        let requested = [clip_id.to_string()];
        let source = self
            .get_clips(&requested)
            .await?
            .into_iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| CliError::NotFound(format!("clip: {clip_id}")))?;

        let mut req = GenerateRequest::new("chirp-v3-0", "custom");
        req.task = Some("gen_stem".into());
        req.title = Some(source.title);
        req.make_instrumental = true;
        req.continue_clip_id = Some(clip_id.to_string());
        req.stem_type_id = Some(91);
        req.stem_type_group_name = Some("Twelve".into());
        req.stem_task = Some("twelve".into());
        req.metadata.is_remix = Some(true);
        req.set_challenge_token(challenge_token);

        self.generate(&req).await
    }
}
