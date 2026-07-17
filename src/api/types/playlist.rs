use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::clip::Clip;

#[derive(Debug, Deserialize, Serialize)]
pub struct PlaylistListResponse {
    #[serde(default, alias = "numTotalResults")]
    pub num_total_results: u64,
    #[serde(default, alias = "currentPage")]
    pub current_page: u32,
    #[serde(default)]
    pub playlists: Vec<PlaylistInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub cover_url: Option<String>,
    pub cover_image_s3_id: Option<String>,
    pub cover_is_user_set: Option<bool>,
    pub is_public: bool,
    pub is_trashed: bool,
    pub song_count: Option<u64>,
    pub num_total_results: Option<u64>,
    pub clip_ids: Vec<String>,
    pub playlist_clips: Vec<PlaylistClip>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relationship: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<Value>,
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawPlaylistInfo {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub is_public: Option<bool>,
    #[serde(default)]
    pub is_trashed: Option<bool>,
    #[serde(default)]
    pub song_count: Option<u64>,
    #[serde(default, alias = "numTotalResults")]
    pub num_total_results: Option<u64>,
    #[serde(default, alias = "clipIds")]
    pub clip_ids: Vec<String>,
    #[serde(default)]
    pub playlist_clips: Vec<PlaylistClip>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    relationship: Option<Value>,
    #[serde(default)]
    stats: Option<Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl<'de> Deserialize<'de> for PlaylistInfo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut raw = RawPlaylistInfo::deserialize(deserializer)?;
        let metadata = raw.metadata.take();
        let relationship = raw.relationship.take();
        let stats = raw.stats.take();
        let cover_url =
            string_field(metadata.as_ref(), "cover_url").or_else(|| raw.image_url.clone());
        let image_url = raw
            .image_url
            .or_else(|| cover_url.clone())
            .or_else(|| string_field(metadata.as_ref(), "image_url"));

        Ok(Self {
            id: raw
                .id
                .or_else(|| string_field(metadata.as_ref(), "id"))
                .ok_or_else(|| D::Error::missing_field("id"))?,
            name: raw
                .name
                .filter(|name| !name.trim().is_empty())
                .or_else(|| string_field(metadata.as_ref(), "name"))
                .unwrap_or_default(),
            description: raw
                .description
                .or_else(|| string_field(metadata.as_ref(), "description")),
            image_url,
            cover_url,
            cover_image_s3_id: string_field(metadata.as_ref(), "cover_image_s3_id"),
            cover_is_user_set: bool_field(metadata.as_ref(), "cover_is_user_set"),
            is_public: raw
                .is_public
                .or_else(|| bool_field(metadata.as_ref(), "is_public"))
                .unwrap_or(false),
            is_trashed: raw
                .is_trashed
                .or_else(|| bool_field(metadata.as_ref(), "is_trashed"))
                .or_else(|| bool_field(relationship.as_ref(), "is_trashed"))
                .unwrap_or(false),
            song_count: raw
                .song_count
                .or_else(|| u64_field(metadata.as_ref(), "song_count"))
                .or_else(|| u64_field(stats.as_ref(), "track_count")),
            num_total_results: raw.num_total_results,
            clip_ids: raw.clip_ids,
            playlist_clips: raw.playlist_clips,
            metadata,
            relationship,
            stats,
            extra: raw.extra,
        })
    }
}

fn string_field(object: Option<&Value>, field: &str) -> Option<String> {
    object?.get(field)?.as_str().map(ToOwned::to_owned)
}

fn bool_field(object: Option<&Value>, field: &str) -> Option<bool> {
    object?.get(field)?.as_bool()
}

fn u64_field(object: Option<&Value>, field: &str) -> Option<u64> {
    object?.get(field)?.as_u64()
}

impl PlaylistInfo {
    pub fn clip_count(&self) -> u64 {
        self.song_count
            .or(self.num_total_results)
            .unwrap_or_else(|| self.playlist_clips.len().max(self.clip_ids.len()) as u64)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaylistClip {
    #[serde(default)]
    pub clip: Option<Clip>,
    #[serde(default)]
    pub relative_index: Option<f64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct CreatePlaylistRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SetPlaylistMetadataRequest {
    pub playlist_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PlaylistTracksRequest {
    pub clip_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistTrackMutationReport {
    pub playlist_id: String,
    pub action: String,
    pub requested_clip_ids: Vec<String>,
    pub succeeded_clip_ids: Vec<String>,
    pub failed: Vec<PlaylistTrackMutationFailure>,
    pub not_attempted_clip_ids: Vec<String>,
}

impl PlaylistTrackMutationReport {
    pub fn new(
        playlist_id: &str,
        action: &str,
        requested_clip_ids: &[String],
        succeeded_clip_ids: Vec<String>,
        failed: Vec<PlaylistTrackMutationFailure>,
        not_attempted_clip_ids: Vec<String>,
    ) -> Self {
        Self {
            playlist_id: playlist_id.to_string(),
            action: action.to_string(),
            requested_clip_ids: requested_clip_ids.to_vec(),
            succeeded_clip_ids,
            failed,
            not_attempted_clip_ids,
        }
    }

    pub fn is_success(&self) -> bool {
        self.failed.is_empty() && self.not_attempted_clip_ids.is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaylistTrackMutationFailure {
    pub clip_id: String,
    pub error_code: String,
    pub message: String,
}

impl PlaylistTrackMutationFailure {
    pub fn from_error(clip_id: &str, error: &crate::core::CliError) -> Self {
        Self {
            clip_id: clip_id.to_string(),
            error_code: error.error_code().to_string(),
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SetPlaylistVisibilityRequest {
    pub metadata: PlaylistVisibilityMetadata,
}

impl SetPlaylistVisibilityRequest {
    pub fn new(is_public: bool) -> Self {
        Self {
            metadata: PlaylistVisibilityMetadata { is_public },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlaylistVisibilityMetadata {
    pub is_public: bool,
}

#[derive(Debug, Serialize)]
pub struct SetPlaylistCoverRequest {
    pub metadata: PlaylistCoverMetadata,
}

impl SetPlaylistCoverRequest {
    pub fn from_upload_id(upload_id: &str) -> Self {
        let cover_image_s3_id = format!("image_{upload_id}");
        Self {
            metadata: PlaylistCoverMetadata {
                cover_url: format!("https://cdn2.suno.ai/{cover_image_s3_id}.jpeg"),
                cover_image_s3_id,
                cover_is_user_set: true,
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlaylistCoverMetadata {
    pub cover_url: String,
    pub cover_image_s3_id: String,
    pub cover_is_user_set: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum PlaylistReaction {
    Like,
    Dislike,
}

impl PlaylistReaction {
    pub fn as_api_value(self) -> &'static str {
        match self {
            Self::Like => "LIKE",
            Self::Dislike => "DISLIKE",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SetPlaylistReactionRequest {
    pub reaction: Option<String>,
}

impl SetPlaylistReactionRequest {
    pub fn new(reaction: Option<PlaylistReaction>) -> Self {
        Self {
            reaction: reaction.map(|reaction| reaction.as_api_value().to_string()),
        }
    }

    #[cfg(test)]
    pub fn like() -> Self {
        Self::new(Some(PlaylistReaction::Like))
    }

    #[cfg(test)]
    pub fn clear() -> Self {
        Self::new(None)
    }
}

#[derive(Debug, Serialize)]
pub struct PlaylistReorderRequest {
    pub positions: Vec<PlaylistTrackPosition>,
}

impl PlaylistReorderRequest {
    pub fn single(clip_id: impl Into<String>, index: u32) -> Self {
        Self {
            positions: vec![PlaylistTrackPosition {
                clip_id: clip_id.into(),
                index,
            }],
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlaylistTrackPosition {
    pub clip_id: String,
    pub index: u32,
}

#[derive(Debug, Serialize)]
pub struct TrashPlaylistRequest {
    pub undo: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        CreatePlaylistRequest, PlaylistInfo, PlaylistReorderRequest, PlaylistTracksRequest,
        SetPlaylistCoverRequest, SetPlaylistMetadataRequest, SetPlaylistReactionRequest,
        SetPlaylistVisibilityRequest, TrashPlaylistRequest,
    };

    #[test]
    fn playlist_info_accepts_v2_deferred_metadata_shape() {
        let playlist: PlaylistInfo = serde_json::from_value(serde_json::json!({
            "metadata": {
                "id": "playlist-1",
                "name": "番茄",
                "description": null,
                "song_count": 9,
                "cover_url": "https://cdn2.suno.ai/image_cover.jpeg",
                "cover_image_s3_id": "image_cover",
                "cover_is_user_set": false,
                "is_public": false,
                "cover_image_grid": ["https://cdn2.suno.ai/image_cover.jpeg"],
                "owner": {
                    "display_name": "Owner",
                    "handle": "owner-handle"
                },
                "created_at": "2026-07-16T09:25:33.434Z"
            },
            "relationship": {
                "is_trashed": false,
                "is_owned": true,
                "can_edit": true
            },
            "stats": {
                "track_count": 9,
                "save_count": 2,
                "total_duration_seconds": 2056
            },
            "bio": {},
            "deferred_fields": []
        }))
        .expect("deserialize deferred playlist response");

        assert_eq!(playlist.id, "playlist-1");
        assert_eq!(playlist.name, "番茄");
        assert_eq!(playlist.song_count, Some(9));
        assert_eq!(playlist.clip_count(), 9);
        assert!(!playlist.is_public);
        assert!(!playlist.is_trashed);

        let output = serde_json::to_value(&playlist).expect("serialize normalized playlist");
        assert_eq!(output["metadata"]["owner"]["handle"], "owner-handle");
        assert!(output["metadata"].get("description").is_some());
        assert!(output["metadata"]["description"].is_null());
        assert_eq!(
            output["metadata"]["cover_image_grid"][0],
            "https://cdn2.suno.ai/image_cover.jpeg"
        );
        assert_eq!(output["relationship"]["is_owned"], true);
        assert_eq!(output["relationship"]["can_edit"], true);
        assert_eq!(output["stats"]["save_count"], 2);
        assert_eq!(output["stats"]["total_duration_seconds"], 2056);
        assert_eq!(output["extra"]["bio"], serde_json::json!({}));
        assert_eq!(output["extra"]["deferred_fields"], serde_json::json!([]));
    }

    #[test]
    fn create_playlist_request_matches_web_shape() {
        let req = CreatePlaylistRequest {
            name: "Mixtape".into(),
        };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(json, serde_json::json!({ "name": "Mixtape" }));
    }

    #[test]
    fn set_playlist_metadata_omits_absent_fields() {
        let req = SetPlaylistMetadataRequest {
            playlist_id: "playlist-1".into(),
            name: Some("Renamed".into()),
            description: None,
            image_url: None,
        };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({
                "playlist_id": "playlist-1",
                "name": "Renamed"
            })
        );
    }

    #[test]
    fn playlist_track_request_uses_clip_ids() {
        let req = PlaylistTracksRequest {
            clip_ids: vec!["clip-a".into(), "clip-b".into()],
        };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({ "clip_ids": ["clip-a", "clip-b"] })
        );
    }

    #[test]
    fn trash_playlist_request_uses_undo_flag() {
        let req = TrashPlaylistRequest { undo: false };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(json, serde_json::json!({ "undo": false }));
    }

    #[test]
    fn restore_playlist_request_sets_undo_flag() {
        let req = TrashPlaylistRequest { undo: true };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(json, serde_json::json!({ "undo": true }));
    }

    #[test]
    fn set_playlist_visibility_uses_v2_metadata_shape() {
        let req = SetPlaylistVisibilityRequest::new(false);

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({ "metadata": { "is_public": false } })
        );
    }

    #[test]
    fn set_playlist_cover_uses_v2_metadata_shape() {
        let req = SetPlaylistCoverRequest::from_upload_id("upload-1");

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({
                "metadata": {
                    "cover_url": "https://cdn2.suno.ai/image_upload-1.jpeg",
                    "cover_image_s3_id": "image_upload-1",
                    "cover_is_user_set": true
                }
            })
        );
    }

    #[test]
    fn reorder_playlist_request_uses_positions_array() {
        let req = PlaylistReorderRequest::single("clip-a", 3);

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({ "positions": [{ "clip_id": "clip-a", "index": 3 }] })
        );
    }

    #[test]
    fn set_playlist_metadata_includes_image_url() {
        let req = SetPlaylistMetadataRequest {
            playlist_id: "playlist-1".into(),
            name: None,
            description: None,
            image_url: Some("https://cdn.example/cover.jpg".into()),
        };

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(
            json,
            serde_json::json!({
                "playlist_id": "playlist-1",
                "image_url": "https://cdn.example/cover.jpg"
            })
        );
    }

    #[test]
    fn playlist_like_request_matches_web_shape() {
        let req = SetPlaylistReactionRequest::like();

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(json, serde_json::json!({ "reaction": "LIKE" }));
    }

    #[test]
    fn playlist_clear_reaction_request_matches_web_shape() {
        let req = SetPlaylistReactionRequest::clear();

        let json = serde_json::to_value(req).expect("serialize request");

        assert_eq!(json, serde_json::json!({ "reaction": null }));
    }
}
