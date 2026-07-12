use serde::{Deserialize, Serialize};

use super::clip::Clip;
use super::prompts::PromptUpsampleResponse;

const WEB_CLIENT_PATHNAME: &str = "/create";
const GENERATION_TYPE_TEXT: &str = "TEXT";
const CHALLENGE_TOKEN_PROVIDER: u8 = 1;
const TAG_UPSAMPLE_PERSONALIZATION_ENABLED: bool = true;

/// Shared browser-facing generation fields that are common across create,
/// cover, extend, stems, and other `/api/generate/v2-web/` submits.
#[derive(Debug, Clone, Default)]
pub struct GenerationWebContext {
    pub user_tier: Option<String>,
}

impl GenerationWebContext {
    fn user_tier_value(&self) -> String {
        self.user_tier
            .as_deref()
            .map(str::trim)
            .filter(|tier| !tier.is_empty())
            .unwrap_or_default()
            .to_string()
    }
}

/// Schema used by Suno's web generation endpoint `/api/generate/v2-web/`.
/// Placeholder fields must be present or Suno's server-side schema rejects
/// the request.
#[derive(Debug, Clone, Serialize)]
pub struct GenerateRequest {
    /// Optional anti-bot challenge token. Suno accepts many authenticated
    /// generation requests without one; callers can still force or supply a
    /// solved token when an account/session is challenged.
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    pub generation_type: String,
    pub title: Option<String>,
    pub tags: Option<String>,
    /// Always present, defaults to an empty string.
    pub negative_tags: String,
    pub mv: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gpt_description_prompt: Option<String>,
    pub make_instrumental: bool,
    pub user_uploaded_images_b64: Option<String>,
    pub metadata: GenerateMetadata,
    /// Always present, empty array unless overriding model fields.
    pub override_fields: Vec<String>,
    pub cover_clip_id: Option<String>,
    pub cover_start_s: Option<f64>,
    pub cover_end_s: Option<f64>,
    pub persona_id: Option<String>,
    pub artist_clip_id: Option<String>,
    pub artist_start_s: Option<f64>,
    pub artist_end_s: Option<f64>,
    pub continue_clip_id: Option<String>,
    pub continued_aligned_prompt: Option<String>,
    pub continue_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playlist_clip_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stem_type_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stem_type_group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stem_task: Option<String>,
    /// Random UUID generated per request.
    pub transaction_uuid: String,
    pub token_provider: Option<u8>,
}

impl GenerateRequest {
    pub fn new(mv: &str, create_mode: &str) -> Self {
        Self::new_with_context(mv, create_mode, &GenerationWebContext::default())
    }

    pub fn new_with_context(mv: &str, create_mode: &str, context: &GenerationWebContext) -> Self {
        Self {
            token: None,
            task: None,
            generation_type: GENERATION_TYPE_TEXT.to_string(),
            title: None,
            tags: None,
            negative_tags: String::new(),
            mv: mv.to_string(),
            prompt: String::new(),
            gpt_description_prompt: None,
            make_instrumental: false,
            user_uploaded_images_b64: None,
            metadata: GenerateMetadata::new_with_context(create_mode, context),
            override_fields: Vec::new(),
            cover_clip_id: None,
            cover_start_s: None,
            cover_end_s: None,
            persona_id: None,
            artist_clip_id: None,
            artist_start_s: None,
            artist_end_s: None,
            continue_clip_id: None,
            continued_aligned_prompt: None,
            continue_at: None,
            playlist_id: None,
            playlist_clip_ids: None,
            stem_type_id: None,
            stem_type_group_name: None,
            stem_task: None,
            transaction_uuid: uuid::Uuid::new_v4().to_string(),
            token_provider: None,
        }
    }

    pub fn set_challenge_token(&mut self, token: Option<String>) {
        self.token = token;
        self.token_provider = self.token.as_ref().map(|_| CHALLENGE_TOKEN_PROVIDER);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerateMetadata {
    pub web_client_pathname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_max_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_mumble: Option<bool>,
    pub create_mode: String,
    pub user_tier: String,
    /// Random UUID generated per request.
    pub create_session_token: String,
    pub disable_volume_normalization: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_sliders: Option<ControlSliders>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_remix: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_updated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tags_generation: Option<LastTagsGeneration>,
}

impl GenerateMetadata {
    fn new_with_context(create_mode: &str, context: &GenerationWebContext) -> Self {
        Self {
            web_client_pathname: WEB_CLIENT_PATHNAME.to_string(),
            is_max_mode: Some(false),
            is_mumble: Some(false),
            create_mode: create_mode.to_string(),
            user_tier: context.user_tier_value(),
            create_session_token: uuid::Uuid::new_v4().to_string(),
            disable_volume_normalization: false,
            control_sliders: None,
            lyrics_model: None,
            is_remix: None,
            lyrics_updated: None,
            last_tags_generation: None,
        }
    }

    pub fn omit_create_form_flags(&mut self) {
        self.is_max_mode = None;
        self.is_mumble = None;
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LastTagsGeneration {
    pub tags: String,
    pub request_id: String,
    pub original_tags: String,
    pub personalization_enabled: bool,
}

impl LastTagsGeneration {
    pub fn from_upsample_response(original_tags: String, response: PromptUpsampleResponse) -> Self {
        Self {
            tags: response.upsampled,
            request_id: response.request_id,
            original_tags,
            // Captured web submits set this field to true when carrying
            // tag-upsample metadata; it is not returned by /api/prompts/upsample.
            personalization_enabled: TAG_UPSAMPLE_PERSONALIZATION_ENABLED,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlSliders {
    /// Weirdness: 0.0-1.0 (maps from 0-100 in UI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weirdness_constraint: Option<f64>,
    /// Style weight: 0.0-1.0 (maps from 0-100 in this CLI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style_weight: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateResponse {
    #[serde(default)]
    pub clips: Option<Vec<Clip>>,
}

impl GenerateResponse {
    pub fn into_clips(self) -> Result<Vec<Clip>, crate::core::CliError> {
        match self.clips {
            Some(clips) if !clips.is_empty() => Ok(clips),
            clips => Err(crate::core::CliError::SunoApi {
                code: "schema_drift",
                status: 200,
                message: "HTTP 200 generation response did not contain any clips".into(),
                retryable: Some(false),
                details: Some(serde_json::json!({
                    "http_status": 200,
                    "response_field": "clips",
                    "field_state": if clips.is_some() { "empty" } else { "missing" }
                })),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_context_sets_shared_web_metadata() {
        let context = GenerationWebContext {
            user_tier: Some("tier-pro".into()),
        };

        let request = GenerateRequest::new_with_context("chirp-fenix", "custom", &context);
        let body = serde_json::to_value(request).expect("request json");

        assert_eq!(body["generation_type"], "TEXT");
        assert_eq!(body["metadata"]["web_client_pathname"], "/create");
        assert_eq!(body["metadata"]["user_tier"], "tier-pro");
        assert!(body["metadata"]["create_session_token"].as_str().is_some());
        assert!(body["transaction_uuid"].as_str().is_some());
    }

    #[test]
    fn generation_metadata_can_carry_real_tag_upsample_response() {
        let mut request = GenerateRequest::new("chirp-fenix", "custom");
        request.tags = Some("garage pop, dry drums".into());
        request.metadata.last_tags_generation = Some(LastTagsGeneration {
            tags: "garage pop, dry drums".into(),
            request_id: "request-1".into(),
            original_tags: "garage pop".into(),
            personalization_enabled: true,
        });

        let body = serde_json::to_value(request).expect("request json");

        assert_eq!(
            body["metadata"]["last_tags_generation"]["tags"],
            body["tags"]
        );
        assert_eq!(
            body["metadata"]["last_tags_generation"]["request_id"],
            "request-1"
        );
        assert_eq!(
            body["metadata"]["last_tags_generation"]["original_tags"],
            "garage pop"
        );
        assert_eq!(
            body["metadata"]["last_tags_generation"]["personalization_enabled"],
            true
        );
    }

    #[test]
    fn challenge_token_sets_web_token_provider() {
        let mut request = GenerateRequest::new("chirp-fenix", "custom");

        request.set_challenge_token(Some("challenge-token".into()));
        let body = serde_json::to_value(request).expect("request json");

        assert_eq!(body["token"], "challenge-token");
        assert_eq!(body["token_provider"], 1);
    }

    #[test]
    fn generation_response_rejects_missing_or_empty_clips() {
        for body in [r#"{}"#, r#"{"clips":[]}"#] {
            let response: GenerateResponse = serde_json::from_str(body).expect("response json");
            let error = response.into_clips().expect_err("clips must be non-empty");

            assert_eq!(error.error_code(), "schema_drift");
            assert_eq!(error.details().expect("details")["http_status"], 200);
        }
    }
}
