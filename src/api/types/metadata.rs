use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct SetMetadataRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_s3_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_audio_upload_tos_accepted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_image_cover: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remove_video_cover: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SetVisibilityRequest {
    pub is_public: bool,
    pub submit_to_contest: bool,
}
