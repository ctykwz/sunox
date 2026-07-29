use super::SunoClient;
use super::generate::{TAG_UPSAMPLE_FEATURE, validate_generation_lengths_with_limits};
#[cfg(test)]
use super::types::Clip;
use super::types::{ControlSliders, GenerateRequest, LastTagsGeneration, PromptUpsampleRequest};
use crate::core::CliError;

pub struct InspirationOptions<'a> {
    pub clip_id: &'a str,
    pub title: &'a str,
    pub tags: &'a str,
    pub negative_tags: &'a str,
    pub lyrics: &'a str,
    pub weirdness: f64,
    pub challenge_token: Option<String>,
}

impl SunoClient {
    #[cfg(test)]
    pub async fn inspire(&self, options: InspirationOptions<'_>) -> Result<Vec<Clip>, CliError> {
        let req = self.prepare_inspiration_request(options).await?;
        self.submit_prepared_generation(&req).await
    }

    pub(crate) async fn prepare_inspiration_request(
        &self,
        options: InspirationOptions<'_>,
    ) -> Result<GenerateRequest, CliError> {
        let original_tags = options.tags.trim();
        if original_tags.is_empty() {
            return Err(CliError::Config(
                "inspiration generation requires non-empty --tags".into(),
            ));
        }
        let lyrics = options.lyrics.trim();
        let mut req = GenerateRequest::new("chirp-fenix", "custom");
        req.task = Some("playlist_condition".into());
        req.title = Some(options.title.to_string());
        req.tags = Some(original_tags.to_string());
        req.negative_tags = options.negative_tags.to_string();
        req.prompt = options.lyrics.to_string();
        req.metadata.control_sliders = Some(ControlSliders {
            weirdness_constraint: Some((options.weirdness / 100.0).clamp(0.0, 1.0)),
            style_weight: None,
        });
        req.playlist_id = Some("inspiration".into());
        req.playlist_clip_ids = Some(vec![options.clip_id.to_string()]);
        req.set_challenge_token(options.challenge_token);
        let limits = self
            .prepare_generation_request_with_features(&mut req, &[TAG_UPSAMPLE_FEATURE])
            .await?;

        let upsampled = self
            .upsample_tags(PromptUpsampleRequest {
                original_tags,
                lyrics: (!lyrics.is_empty()).then_some(lyrics),
                is_instrumental: false,
                user_guidance: None,
            })
            .await?;
        req.tags = Some(upsampled.upsampled.clone());
        req.metadata.last_tags_generation = Some(LastTagsGeneration::from_upsample_response(
            original_tags.to_string(),
            upsampled,
        ));
        validate_generation_lengths_with_limits(&req, &limits)?;
        Ok(req)
    }
}
