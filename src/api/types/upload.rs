use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::Clip;

#[derive(Debug, Serialize)]
pub struct CreateAudioUploadRequest {
    pub spec: CreateAudioUploadSpec,
}

#[derive(Debug, Serialize)]
pub struct CreateAudioUploadSpec {
    pub extension: String,
    pub is_stem_mix: bool,
    pub upload_type: String,
}

#[derive(Debug, Deserialize)]
pub struct AudioUploadInitResponse {
    pub id: String,
    pub url: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct CreateImageUploadRequest {
    pub extension: String,
}

#[derive(Debug, Deserialize)]
pub struct ImageUploadInitResponse {
    pub id: String,
    pub url: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct FinishImageUploadResponse {
    pub moderation_status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FinishAudioUploadRequest {
    pub upload_type: String,
    pub upload_filename: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AudioUploadStatus {
    pub id: Option<String>,
    pub status: Option<String>,
    pub title: Option<String>,
    pub image_url: Option<String>,
    pub has_vocal: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct InitializeAudioClipRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub downbeats: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_reviewed_tags: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct InitializeAudioClipResponse {
    pub clip_id: Option<String>,
    pub clip: Option<Clip>,
}
