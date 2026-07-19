use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::clip::Clip;

#[derive(Debug, Clone, Serialize)]
pub struct ClipInfo {
    #[serde(flatten)]
    pub clip: Clip,
    pub attribution: ClipAttribution,
    pub comments: ClipComments,
    pub direct_children_count: u64,
    pub similar_clips: Vec<Clip>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supplemental_errors: Vec<ClipInfoSupplementalError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipInfoSupplementalError {
    pub field: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClipAttribution {
    #[serde(default)]
    pub source_clips: Vec<ClipAttributionSource>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClipAttributionSource {
    #[serde(default)]
    pub clip_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub audio_url: Option<String>,
    #[serde(default)]
    pub is_deleted: Option<bool>,
    #[serde(default)]
    pub relationship: Option<String>,
    #[serde(default)]
    pub user: Option<ClipAttributionUser>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClipAttributionUser {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub user_display_name: Option<String>,
    #[serde(default)]
    pub user_handle: Option<String>,
    #[serde(default)]
    pub user_avatar_url: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClipComments {
    #[serde(default)]
    pub results: Vec<ClipComment>,
    #[serde(default)]
    pub allow_comment: bool,
    #[serde(default)]
    pub total_count: u64,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ClipComment {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub clip_id: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub user_display_name: Option<String>,
    #[serde(default)]
    pub user_handle: Option<String>,
    #[serde(default)]
    pub user_avatar_url: Option<String>,
    #[serde(default)]
    pub user_is_verified: Option<bool>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub num_likes: Option<u64>,
    #[serde(default)]
    pub num_replies: Option<u64>,
    #[serde(default)]
    pub track_timestamp: Option<f64>,
    #[serde(default)]
    pub replies: Vec<serde_json::Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DirectChildrenCountResponse {
    #[serde(default)]
    pub count: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SimilarClipsResponse {
    #[serde(default)]
    pub similar_clips: Vec<Clip>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn song_page_supplemental_unknown_fields_round_trip_in_place() {
        let attribution: ClipAttribution = serde_json::from_value(serde_json::json!({
            "source_clips": [{
                "clip_id": "source-1",
                "relationship": "cover",
                "source_kind": "uploaded",
                "user": {"user_id": "user-1", "badge": "verified"}
            }],
            "attribution_version": 2
        }))
        .expect("attribution");
        let comments: ClipComments = serde_json::from_value(serde_json::json!({
            "results": [{"id": "comment-1", "content": "Great", "moderation_state": "clean"}],
            "allow_comment": true,
            "total_count": 1,
            "next_cursor": "cursor-2"
        }))
        .expect("comments");

        let attribution = serde_json::to_value(attribution).expect("serialize attribution");
        let comments = serde_json::to_value(comments).expect("serialize comments");
        assert_eq!(attribution["attribution_version"], 2);
        assert_eq!(attribution["source_clips"][0]["source_kind"], "uploaded");
        assert_eq!(attribution["source_clips"][0]["user"]["badge"], "verified");
        assert_eq!(comments["next_cursor"], "cursor-2");
        assert_eq!(comments["results"][0]["moderation_state"], "clean");
    }
}
