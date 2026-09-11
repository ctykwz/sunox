use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::challenge::ChallengeProvider;

use super::clip::Clip;
use super::prompts::PromptUpsampleResponse;

const WEB_CLIENT_PATHNAME: &str = "/create";
const GENERATION_TYPE_TEXT: &str = "TEXT";

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_source: Option<String>,
    pub generation_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    /// Always present, defaults to an empty string.
    pub negative_tags: String,
    pub mv: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyricist_id: Option<String>,
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
    pub underpainting_clip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overpainting_clip_id: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stem_name: Option<String>,
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
            edit_session_id: None,
            project_id: None,
            creation_source: None,
            generation_type: GENERATION_TYPE_TEXT.to_string(),
            title: None,
            tags: None,
            negative_tags: String::new(),
            mv: mv.to_string(),
            prompt: String::new(),
            duration: None,
            lyrics_project_id: None,
            lyricist_id: None,
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
            underpainting_clip_id: None,
            overpainting_clip_id: None,
            playlist_id: None,
            playlist_clip_ids: None,
            stem_type_id: None,
            stem_type_group_name: None,
            stem_task: None,
            stem_name: None,
            transaction_uuid: uuid::Uuid::new_v4().to_string(),
            token_provider: None,
        }
    }

    pub fn set_challenge_token(&mut self, token: Option<String>) {
        self.set_challenge_token_with_provider(token, ChallengeProvider::HCaptcha);
    }

    pub fn set_challenge_token_with_provider(
        &mut self,
        token: Option<String>,
        provider: ChallengeProvider,
    ) {
        self.token = token;
        self.token_provider = self.token.as_ref().map(|_| provider.token_provider());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GenerateMetadata {
    pub web_client_pathname: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_max_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_mumble: Option<bool>,
    pub create_mode: String,
    pub user_tier: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_studio_project_id: Option<String>,
    /// Random UUID generated per request.
    pub create_session_token: String,
    pub disable_volume_normalization: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_gender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_speech: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backing_music: Option<bool>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_offset: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recreated_from_clip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sound_configs: Option<Value>,
}

impl GenerateMetadata {
    fn new_with_context(create_mode: &str, context: &GenerationWebContext) -> Self {
        Self {
            web_client_pathname: WEB_CLIENT_PATHNAME.to_string(),
            create_surface: None,
            is_max_mode: Some(false),
            is_mumble: None,
            create_mode: create_mode.to_string(),
            user_tier: context.user_tier_value(),
            from_studio_project_id: None,
            create_session_token: uuid::Uuid::new_v4().to_string(),
            disable_volume_normalization: false,
            vocal_gender: None,
            is_speech: None,
            backing_music: None,
            control_sliders: None,
            lyrics_model: None,
            is_remix: None,
            lyrics_updated: None,
            last_tags_generation: None,
            batch_offset: None,
            recreated_from_clip_id: None,
            sound_configs: None,
        }
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
    pub fn from_upsample_response(
        original_tags: String,
        response: PromptUpsampleResponse,
        personalization_enabled: bool,
    ) -> Self {
        Self {
            tags: response.upsampled,
            request_id: response.request_id,
            original_tags,
            personalization_enabled,
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
    /// Audio reference influence: 0.0-1.0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_weight: Option<f64>,
    /// Account-gated Web control; callers must not fabricate a value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aug_creativity: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct GenerateResponse {
    #[serde(default)]
    pub clips: Option<Vec<Clip>>,
}

#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub clips: Vec<Clip>,
    raw: Value,
}

impl Serialize for GenerationResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.raw.serialize(serializer)
    }
}

impl GenerationResult {
    pub(crate) fn from_clip(clip: Clip, raw: Value) -> Self {
        Self {
            clips: vec![clip],
            raw,
        }
    }
}

impl GenerateResponse {
    pub fn into_result(self, raw: Value) -> Result<GenerationResult, crate::core::CliError> {
        match self.clips {
            Some(clips) if clips.is_empty() => Err(crate::core::CliError::SunoApi {
                code: "schema_drift",
                status: 200,
                message: "HTTP 200 generation response did not contain any clips".into(),
                retryable: Some(false),
                details: Some(serde_json::json!({
                    "http_status": 200,
                    "response_field": "clips",
                    "field_state": "empty"
                })),
            }),
            None => Err(crate::core::CliError::SunoApi {
                code: "schema_drift",
                status: 200,
                message: "HTTP 200 generation response did not contain any clips".into(),
                retryable: Some(false),
                details: Some(serde_json::json!({
                    "http_status": 200,
                    "response_field": "clips",
                    "field_state": "missing"
                })),
            }),
            Some(clips) => {
                if let Some(index) = clips.iter().position(|clip| clip.id.trim().is_empty()) {
                    return Err(crate::core::CliError::SunoApi {
                        code: "schema_drift",
                        status: 200,
                        message: format!(
                            "HTTP 200 generation response contained an empty clip ID at index {index}"
                        ),
                        retryable: Some(false),
                        details: Some(serde_json::json!({
                            "http_status": 200,
                            "response_field": format!("clips[{index}].id"),
                            "field_state": "empty_or_whitespace"
                        })),
                    });
                }
                Ok(GenerationResult { clips, raw })
            }
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
        assert!(body["token"].is_null());
        assert!(body["token_provider"].is_null());
        assert_eq!(body["metadata"]["is_max_mode"], false);
        for omitted in ["title", "tags"] {
            assert!(
                !body
                    .as_object()
                    .expect("request object")
                    .contains_key(omitted),
                "{omitted} should follow the Web client's undefined-field semantics"
            );
        }
        assert!(
            !body["metadata"]
                .as_object()
                .expect("metadata object")
                .contains_key("is_mumble"),
            "is_mumble should be absent when the mode is disabled"
        );
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
    fn challenge_token_preserves_detected_provider() {
        use crate::api::challenge::ChallengeProvider;

        let mut request = GenerateRequest::new("chirp-fenix", "custom");
        request.set_challenge_token_with_provider(
            Some("turnstile-token".into()),
            ChallengeProvider::Turnstile,
        );

        let body = serde_json::to_value(request).expect("request body");
        assert_eq!(body["token"], "turnstile-token");
        assert_eq!(body["token_provider"], 2);
    }

    #[test]
    fn generation_request_serializes_current_optional_web_context() {
        let mut request = GenerateRequest::new("chirp-fenix", "custom");
        request.edit_session_id = Some("edit-1".into());
        request.project_id = Some("project-1".into());
        request.creation_source = Some("cli".into());
        request.duration = Some(120.0);
        request.lyrics_project_id = Some("lyrics-project-1".into());
        request.lyricist_id = Some("lyricist-1".into());
        request.stem_name = Some("Vocals".into());
        request.metadata.create_surface = Some("persistent_panel".into());
        request.metadata.from_studio_project_id = Some("studio-1".into());
        request.metadata.is_speech = Some(true);
        request.metadata.backing_music = Some(false);
        request.metadata.batch_offset = Some(2);
        request.metadata.recreated_from_clip_id = Some("clip-source".into());
        request.metadata.sound_configs = Some(serde_json::json!({"seed": 7}));
        request.metadata.control_sliders = Some(ControlSliders {
            weirdness_constraint: Some(0.4),
            style_weight: Some(0.7),
            audio_weight: Some(0.6),
            aug_creativity: Some(0.25),
        });

        let body = serde_json::to_value(request).expect("request json");

        assert_eq!(body["edit_session_id"], "edit-1");
        assert_eq!(body["project_id"], "project-1");
        assert_eq!(body["creation_source"], "cli");
        assert_eq!(body["duration"], 120.0);
        assert_eq!(body["lyrics_project_id"], "lyrics-project-1");
        assert_eq!(body["lyricist_id"], "lyricist-1");
        assert_eq!(body["stem_name"], "Vocals");
        assert_eq!(body["metadata"]["create_surface"], "persistent_panel");
        assert_eq!(body["metadata"]["from_studio_project_id"], "studio-1");
        assert_eq!(body["metadata"]["is_speech"], true);
        assert_eq!(body["metadata"]["backing_music"], false);
        assert_eq!(body["metadata"]["batch_offset"], 2);
        assert_eq!(body["metadata"]["recreated_from_clip_id"], "clip-source");
        assert_eq!(body["metadata"]["sound_configs"]["seed"], 7);
        assert_eq!(body["metadata"]["control_sliders"]["audio_weight"], 0.6);
        assert_eq!(body["metadata"]["control_sliders"]["aug_creativity"], 0.25);
    }

    #[test]
    fn generation_response_rejects_missing_or_empty_clips() {
        for body in [r#"{}"#, r#"{"clips":[]}"#] {
            let raw: Value = serde_json::from_str(body).expect("raw response json");
            let response: GenerateResponse =
                serde_json::from_value(raw.clone()).expect("response json");
            let error = response
                .into_result(raw)
                .expect_err("clips must be non-empty");

            assert_eq!(error.error_code(), "schema_drift");
            assert_eq!(error.details().expect("details")["http_status"], 200);
        }
    }

    #[test]
    fn generation_response_json_preserves_the_exact_upstream_envelope() {
        let raw = serde_json::json!({
            "id": null,
            "clip_review_prompt_id": "review-1",
            "server_trace": {"region": "iad"},
            "clips": [{
                "id": "clip-1",
                "title": "Demo",
                "status": "submitted",
                "model_name": "chirp-fenix",
                "created_at": "2026-07-27T00:00:00Z"
            }]
        });
        let response: GenerateResponse =
            serde_json::from_value(raw.clone()).expect("response json");

        let result = response
            .into_result(raw.clone())
            .expect("generation result");
        let output = serde_json::to_value(result).expect("result json");

        assert_eq!(output, raw);
        assert!(output["id"].is_null());
        assert!(output["clips"][0].get("audio_url").is_none());
        assert!(output["clips"][0].get("play_count").is_none());
        assert!(output["clips"][0].get("metadata").is_none());
    }
}
