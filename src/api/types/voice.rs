use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct VoicePhrase {
    pub phrase_id: String,
    pub phrase_text: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessedVoiceStatus {
    #[serde(default)]
    pub id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub voice_recording_id: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct ProcessVoiceSampleRequest {
    pub upload_id: String,
    pub vocal_start_s: f64,
    pub vocal_end_s: f64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessVoiceSampleResponse {
    pub id: String,
    pub voice_recording_id: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct ProcessVoiceVerificationRecordingRequest {
    pub upload_id: String,
    pub recording_type: &'static str,
}

impl ProcessVoiceVerificationRecordingRequest {
    pub fn new(upload_id: String) -> Self {
        Self {
            upload_id,
            recording_type: "verification",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ProcessVoiceVerificationRecordingResponse {
    #[serde(default)]
    pub id: Option<String>,
    pub voice_recording_id: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct CreateVoiceVerificationRequest {
    pub voice_recording_id: String,
    pub verification_recording_id: String,
    pub phrase_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct VoiceVerification {
    pub id: String,
    pub status: String,
    #[serde(default)]
    pub rejection_reason: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::{
        CreateVoiceVerificationRequest, ProcessVoiceSampleRequest,
        ProcessVoiceVerificationRecordingRequest,
    };

    #[test]
    fn main_sample_request_matches_current_web_body() {
        let request = ProcessVoiceSampleRequest {
            upload_id: "upload-main".into(),
            vocal_start_s: 0.0,
            vocal_end_s: 37.46,
        };

        assert_eq!(
            serde_json::to_value(request).expect("serialize request"),
            serde_json::json!({
                "upload_id": "upload-main",
                "vocal_start_s": 0.0,
                "vocal_end_s": 37.46
            })
        );
    }

    #[test]
    fn verification_recording_request_uses_the_fixed_recording_type() {
        let request = ProcessVoiceVerificationRecordingRequest::new("upload-verify".into());

        assert_eq!(
            serde_json::to_value(request).expect("serialize request"),
            serde_json::json!({
                "upload_id": "upload-verify",
                "recording_type": "verification"
            })
        );
    }

    #[test]
    fn verification_request_keeps_all_three_server_identities() {
        let request = CreateVoiceVerificationRequest {
            voice_recording_id: "recording-main".into(),
            verification_recording_id: "recording-verify".into(),
            phrase_id: "phrase-1".into(),
        };

        assert_eq!(
            serde_json::to_value(request).expect("serialize request"),
            serde_json::json!({
                "voice_recording_id": "recording-main",
                "verification_recording_id": "recording-verify",
                "phrase_id": "phrase-1"
            })
        );
    }
}
