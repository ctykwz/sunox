use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::clip::Clip;

#[derive(Clone, Copy, Debug)]
pub enum PersonaListScope {
    Mine,
    Loved,
    Followed,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PersonaListResponse {
    #[serde(default)]
    pub personas: Vec<PersonaInfo>,
    #[serde(default, alias = "totalResults")]
    pub total_results: u64,
    #[serde(default, alias = "currentPage")]
    pub current_page: u32,
    #[serde(default, alias = "continuationToken")]
    pub continuation_token: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PersonaClipsResponse {
    pub persona: PersonaInfo,
    #[serde(default)]
    pub total_results: u64,
    #[serde(default)]
    pub current_page: u32,
    #[serde(default)]
    pub is_following: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PersonaClipEntry {
    pub clip: Clip,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TogglePersonaLoveResponse {
    pub loved: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TrashPersonasResponse {
    #[serde(default)]
    pub updated_persona_ids: Vec<String>,
    #[serde(default)]
    pub voice_persona_count: u64,
    #[serde(default)]
    pub max_voice_personas: u64,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct CreatePersonaRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root_clip_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_s3_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_suno_persona: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vox_audio_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_start_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_end_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input_styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub singer_skill_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clips: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_voice_recording: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_recording_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct EditPersonaRequest {
    pub persona_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persona_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_input_styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vox_audio_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_start_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vocal_end_s: Option<f64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessedClipInfo {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub vocal_start_s: Option<f64>,
    #[serde(default)]
    pub vocal_end_s: Option<f64>,
    #[serde(default)]
    pub vocal_audio_url: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::{CreatePersonaRequest, EditPersonaRequest, PersonaInfo, TogglePersonaLoveResponse};

    #[test]
    fn toggle_love_response_reads_loved_state() {
        let response: TogglePersonaLoveResponse =
            serde_json::from_value(serde_json::json!({ "loved": true }))
                .expect("deserialize response");

        assert!(response.loved);
    }

    #[test]
    fn persona_detail_reads_direct_current_web_shape() {
        let persona: PersonaInfo = serde_json::from_value(serde_json::json!({
            "id": "persona-1",
            "name": "Lead Voice",
            "is_loved": true,
            "is_public": true,
            "is_trashed": false,
            "is_hidden": false,
            "clip_count": 4,
            "follower_count": 2,
            "is_following": true,
            "source": "generated_clip",
            "user_input_styles": "warm soul",
            "vocal_start_s": 0.43,
            "vocal_end_s": 22.56,
            "vocal_clip_id": "processed-1",
            "is_vox_persona": true,
            "user_is_verified": true
        }))
        .expect("deserialize persona");

        assert_eq!(persona.id, "persona-1");
        assert_eq!(persona.name, "Lead Voice");
        assert!(persona.is_loved);
        assert_eq!(persona.is_public, Some(true));
        assert_eq!(persona.clip_count, Some(4));
        assert_eq!(persona.follower_count, Some(2));
        assert!(persona.is_following);
        assert_eq!(persona.source.as_deref(), Some("generated_clip"));
        assert_eq!(persona.user_input_styles.as_deref(), Some("warm soul"));
        assert_eq!(persona.vocal_start_s, Some(0.43));
        assert_eq!(persona.vocal_end_s, Some(22.56));
        assert_eq!(persona.vocal_clip_id.as_deref(), Some("processed-1"));
        let output = serde_json::to_value(persona).expect("serialize persona");
        assert_eq!(output["is_vox_persona"], true);
        assert_eq!(output["user_is_verified"], true);
        assert!(output.get("extra").is_none());
    }

    #[test]
    fn create_persona_request_omits_absent_fields() {
        let req = CreatePersonaRequest {
            root_clip_id: Some("clip-a".into()),
            name: Some("Lead Voice".into()),
            description: None,
            image_s3_id: None,
            is_public: Some(false),
            is_suno_persona: None,
            persona_type: None,
            vox_audio_id: None,
            vocal_start_s: None,
            vocal_end_s: None,
            user_input_styles: None,
            source: None,
            singer_skill_level: None,
            clips: None,
            is_voice_recording: None,
            voice_recording_id: None,
            verification_id: None,
        };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({
                "root_clip_id": "clip-a",
                "name": "Lead Voice",
                "is_public": false
            })
        );
    }

    #[test]
    fn edit_persona_request_omits_absent_fields() {
        let req = EditPersonaRequest {
            persona_id: "persona-1".into(),
            name: Some("Lead Voice".into()),
            description: None,
            is_public: Some(false),
            persona_type: Some("vox".into()),
            user_input_styles: None,
            vox_audio_id: Some("processed-1".into()),
            vocal_start_s: Some(0.43),
            vocal_end_s: None,
        };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({
                "persona_id": "persona-1",
                "name": "Lead Voice",
                "is_public": false,
                "persona_type": "vox",
                "vox_audio_id": "processed-1",
                "vocal_start_s": 0.43
            })
        );
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PersonaInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub image_s3_id: Option<String>,
    #[serde(default)]
    pub user_display_name: Option<String>,
    #[serde(default)]
    pub user_handle: Option<String>,
    #[serde(default)]
    pub user_image_url: Option<String>,
    #[serde(default)]
    pub persona_type: Option<String>,
    #[serde(default)]
    pub root_clip_id: Option<String>,
    #[serde(default)]
    pub is_loved: bool,
    #[serde(default)]
    pub is_owned: bool,
    #[serde(default)]
    pub is_public: Option<bool>,
    #[serde(default)]
    pub is_trashed: bool,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub clip_count: Option<u64>,
    #[serde(default)]
    pub follower_count: Option<u64>,
    #[serde(default)]
    pub is_following: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub user_input_styles: Option<String>,
    #[serde(default)]
    pub vocal_start_s: Option<f64>,
    #[serde(default)]
    pub vocal_end_s: Option<f64>,
    #[serde(default)]
    pub vocal_clip_id: Option<String>,
    #[serde(default)]
    pub clip: Option<Clip>,
    #[serde(default)]
    pub persona_clips: Vec<PersonaClipEntry>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
