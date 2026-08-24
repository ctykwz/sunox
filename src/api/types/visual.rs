use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::core::CliError;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CoverArtPromptImage {
    pub id: String,
    #[serde(rename = "type")]
    pub image_type: String,
}

impl CoverArtPromptImage {
    pub fn uploaded(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            image_type: "uploaded".into(),
        }
    }

    pub fn generated(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            image_type: "generated".into(),
        }
    }

    pub fn s3_filename(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            image_type: "s3_filename".into(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CliError> {
        if self.id.trim().is_empty() {
            return Err(CliError::Config(
                "cover-art prompt image ID must not be empty".into(),
            ));
        }
        if !matches!(
            self.image_type.as_str(),
            "uploaded" | "generated" | "s3_filename"
        ) {
            return Err(CliError::Config(format!(
                "unsupported cover-art prompt image type `{}`",
                self.image_type
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtModelCategory {
    pub category: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub audio: Option<String>,
    #[serde(default)]
    pub allowed_durations: Vec<u32>,
    #[serde(default)]
    pub allowed_durations_with_image: Vec<u32>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtModelConfigs {
    pub image_model_categories: Vec<CoverArtModelCategory>,
    pub video_model_categories: Vec<CoverArtModelCategory>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtCost {
    pub cost: f64,
    #[serde(default)]
    pub remaining_gens: Option<u64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverArtImageGenerateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_text_id: Option<String>,
    pub prompt: String,
    pub clip_id: String,
    pub quantity: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_gen_category: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub prompt_images: Vec<CoverArtPromptImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverArtVideoGenerateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_text_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_start_image: Option<CoverArtPromptImage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_id: Option<String>,
    pub prompt: String,
    pub quantity: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_gen_category: Option<String>,
    pub duration: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_start_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip_end_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtBatchSubmission {
    pub batch_id: String,
    #[serde(default)]
    pub image_ids: Vec<String>,
    #[serde(default)]
    pub video_ids: Vec<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct CoverArtBatchDescriptor {
    pub id: String,
    #[serde(rename = "type")]
    pub media_type: String,
}

impl CoverArtBatchDescriptor {
    pub fn new(id: impl Into<String>, media_type: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            media_type: media_type.into(),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CliError> {
        if self.id.trim().is_empty() {
            return Err(CliError::Config(
                "cover-art batch ID must not be empty".into(),
            ));
        }
        if !matches!(self.media_type.as_str(), "image" | "video") {
            return Err(CliError::Config(
                "cover-art batch type must be image or video".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtPendingBatches {
    pub batch_ids: Vec<CoverArtBatchDescriptor>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CoverArtHistoryRequest {
    pub clip_id: Option<String>,
    pub created_at_offset: Option<String>,
    pub favorites_only: bool,
    pub media_type: Option<String>,
    pub limit: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtBatchItem {
    pub id: String,
    #[serde(default)]
    pub clip_id: Option<String>,
    #[serde(rename = "type")]
    pub media_type: String,
    pub status: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub is_liked: Option<bool>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub video_upload_id: Option<String>,
    #[serde(default)]
    pub start_frame_url: Option<String>,
    #[serde(default)]
    pub gen_category: Option<String>,
    #[serde(default)]
    pub duration: Option<u32>,
    #[serde(default)]
    pub clip_start_time: Option<f64>,
    #[serde(default)]
    pub clip_end_time: Option<f64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl CoverArtBatchItem {
    pub fn is_complete(&self) -> bool {
        self.status == "complete"
    }

    pub fn is_error(&self) -> bool {
        self.status == "error"
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtHistoryBatch {
    pub batch_id: String,
    #[serde(rename = "type")]
    pub media_type: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub gen_category: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub items: Vec<CoverArtBatchItem>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtHistoryResponse {
    pub history: Vec<CoverArtHistoryBatch>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtPollResponse {
    pub batches: BTreeMap<String, Vec<CoverArtBatchItem>>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CoverArtApplyResponse {
    pub image_url: String,
    #[serde(default)]
    pub video_cover_url: Option<String>,
    #[serde(default)]
    pub preview_url: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct PromptImageRequest<'a> {
    pub prompt: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PromptImageResponse {
    #[serde(deserialize_with = "deserialize_nonempty_image_url")]
    pub image_url: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VideoGenerationStatus {
    #[serde(deserialize_with = "deserialize_nonempty_status")]
    pub status: String,
    #[serde(default)]
    pub video_url: Option<String>,
    #[serde(default)]
    pub video_is_stale: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl VideoGenerationStatus {
    pub fn is_complete(&self) -> bool {
        self.status == "complete"
    }

    pub fn is_failure(&self) -> bool {
        matches!(self.status.as_str(), "failed" | "error")
    }

    pub(crate) fn validate(self) -> Result<Self, CliError> {
        if self.is_complete()
            && self
                .video_url
                .as_deref()
                .is_none_or(|url| url.trim().is_empty())
        {
            return Err(CliError::Api {
                code: "schema_drift",
                message: "complete video-generation status had no usable video_url".into(),
            });
        }
        Ok(self)
    }
}

fn deserialize_nonempty_image_url<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "prompt image response image_url must not be empty",
        ));
    }
    Ok(value)
}

fn deserialize_nonempty_status<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "video-generation status must not be empty",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{PromptImageResponse, VideoGenerationStatus};

    #[test]
    fn visual_responses_require_recovery_handles() {
        serde_json::from_value::<PromptImageResponse>(serde_json::json!({
            "image_url": " "
        }))
        .expect_err("blank image URL must be rejected");

        let complete: VideoGenerationStatus = serde_json::from_value(serde_json::json!({
            "status": "complete"
        }))
        .expect("status shape");
        complete
            .validate()
            .expect_err("complete video status requires a URL");
    }
}
