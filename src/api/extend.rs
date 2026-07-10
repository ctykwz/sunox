use super::SunoClient;
use super::types::{Clip, GenerateRequest};
use crate::core::CliError;

pub struct ExtendClipOptions<'a> {
    pub clip_id: &'a str,
    pub continue_at: f64,
    pub tags: Option<&'a str>,
    pub negative_tags: Option<&'a str>,
    pub lyrics: Option<&'a str>,
    pub title: Option<&'a str>,
    pub instrumental: Option<bool>,
    pub challenge_token: Option<String>,
}

impl SunoClient {
    /// Continue an existing clip from a timestamp via the current web generation route.
    #[cfg(test)]
    pub async fn extend(&self, options: ExtendClipOptions<'_>) -> Result<Vec<Clip>, CliError> {
        let req = self.prepare_extend_request(options).await?;
        self.generate(&req).await
    }

    pub(crate) async fn prepare_extend_request(
        &self,
        options: ExtendClipOptions<'_>,
    ) -> Result<GenerateRequest, CliError> {
        crate::core::ensure_non_negative_finite("continue_at", options.continue_at)?;
        let requested = [options.clip_id.to_string()];
        let mut source = self
            .get_clips(&requested)
            .await?
            .into_iter()
            .find(|clip| clip.id == options.clip_id)
            .ok_or_else(|| CliError::NotFound(format!("clip: {}", options.clip_id)))?;
        if extend_source_needs_feed_metadata(&source, &options) {
            match self.search(&source.title).await {
                Ok(feed) => {
                    if let Some(enriched) = feed
                        .clips
                        .into_iter()
                        .find(|clip| clip.id == options.clip_id)
                    {
                        merge_extend_source_metadata(&mut source, enriched);
                    }
                }
                Err(error) if error.is_auth_or_rate_limit() => return Err(error),
                Err(_) => {}
            }
        }

        let mut req = GenerateRequest::new("chirp-fenix", "custom");
        req.task = Some("extend".into());
        req.title = Some(
            options
                .title
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(&source.title)
                .to_string(),
        );
        req.prompt = options.lyrics.unwrap_or_default().to_string();
        req.continued_aligned_prompt = Some(source.metadata.prompt.unwrap_or_default());
        req.tags = Some(
            options
                .tags
                .map(str::to_string)
                .or(source.metadata.tags)
                .unwrap_or_default(),
        );
        req.negative_tags = options
            .negative_tags
            .map(str::to_string)
            .or(source.metadata.negative_tags)
            .unwrap_or_default();
        req.make_instrumental = options
            .instrumental
            .or(source.metadata.make_instrumental)
            .unwrap_or_default();
        req.continue_clip_id = Some(options.clip_id.to_string());
        req.continue_at = Some(options.continue_at);
        req.metadata.is_remix = Some(true);
        req.metadata.lyrics_updated = Some(true);
        req.set_challenge_token(options.challenge_token);

        Ok(req)
    }
}

fn extend_source_needs_feed_metadata(source: &Clip, options: &ExtendClipOptions<'_>) -> bool {
    (options.tags.is_none() && option_string_is_blank(source.metadata.tags.as_ref()))
        || (options.negative_tags.is_none() && source.metadata.negative_tags.is_none())
        || (options.instrumental.is_none() && source.metadata.make_instrumental.is_none())
}

fn option_string_is_blank(value: Option<&String>) -> bool {
    value.map(|value| value.trim().is_empty()).unwrap_or(true)
}

fn merge_extend_source_metadata(source: &mut Clip, enriched: Clip) {
    if option_string_is_blank(source.metadata.tags.as_ref())
        && let Some(tags) = enriched.metadata.tags
    {
        source.metadata.tags = Some(tags);
    }
    if source.metadata.negative_tags.is_none() {
        source.metadata.negative_tags = enriched.metadata.negative_tags;
    }
    if source.metadata.make_instrumental.is_none() {
        source.metadata.make_instrumental = enriched.metadata.make_instrumental;
    }
}
