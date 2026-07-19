use super::SunoClient;
#[cfg(test)]
use super::types::Clip;
use super::types::GenerateRequest;
use crate::core::CliError;

impl SunoClient {
    /// Create a cover of an existing clip.
    /// Posts to `/api/generate/v2-web/` with `cover_clip_id` set. Capture a
    /// fresh web request if Suno starts requiring extra cover fields such as
    /// `cover_start_s` or `cover_end_s`.
    #[cfg(test)]
    pub async fn cover(
        &self,
        clip_id: &str,
        model_key: &str,
        tags: Option<&str>,
        challenge_token: Option<String>,
    ) -> Result<Vec<Clip>, CliError> {
        let req = self
            .prepare_cover_request(clip_id, model_key, tags, challenge_token)
            .await?;
        self.generate(&req).await
    }

    pub(crate) async fn prepare_cover_request(
        &self,
        clip_id: &str,
        model_key: &str,
        tags: Option<&str>,
        challenge_token: Option<String>,
    ) -> Result<GenerateRequest, CliError> {
        let requested = [clip_id.to_string()];
        let source = self
            .get_clips(&requested)
            .await?
            .into_iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| CliError::NotFound(format!("clip: {clip_id}")))?;

        let mut req = GenerateRequest::new(model_key, "custom");
        req.task = Some("cover".into());
        req.title = Some(source.title);
        req.tags = tags.map(String::from);
        req.cover_clip_id = Some(clip_id.to_string());
        req.set_challenge_token(challenge_token);
        Ok(req)
    }
}
