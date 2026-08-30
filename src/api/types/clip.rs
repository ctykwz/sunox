use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Clip {
    pub id: String,
    pub title: String,
    pub status: String,
    pub model_name: String,
    pub audio_url: Option<String>,
    pub video_url: Option<String>,
    pub image_url: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_trashed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_download_unlocked: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_config: Option<ClipActionConfig>,
    #[serde(default)]
    pub play_count: u64,
    #[serde(default)]
    pub upvote_count: u64,
    #[serde(default)]
    pub metadata: ClipMetadata,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Clip {
    /// Return the non-authorizing URL that the current Web player can stream.
    ///
    /// Suno may expose `/api/forbidden` in the legacy top-level `audio_url`
    /// while providing a progressive M4A in `media_urls`. Direct MP3 media
    /// URLs are deliberately not selected here: current accounts can receive
    /// a protected CDN URL that requires the separate download workflow.
    pub fn playback_url(&self) -> Option<&str> {
        self.audio_url
            .as_deref()
            .filter(|url| is_usable_audio_url(url))
            .or_else(|| self.progressive_m4a_url())
    }

    fn progressive_m4a_url(&self) -> Option<&str> {
        self.extra
            .get("media_urls")?
            .as_array()?
            .iter()
            .filter_map(Value::as_object)
            .find_map(|media| {
                let delivery = media.get("delivery")?.as_str()?;
                let content_type = media.get("content_type")?.as_str()?;
                let url = media.get("url")?.as_str()?;
                (delivery.eq_ignore_ascii_case("progressive")
                    && is_m4a_content_type(content_type)
                    && is_usable_audio_url(url))
                .then_some(url)
            })
    }
}

fn is_usable_audio_url(url: &str) -> bool {
    let url = url.trim();
    if url.is_empty() {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    !path.ends_with("/api/forbidden")
}

fn is_m4a_content_type(content_type: &str) -> bool {
    let content_type = content_type.trim();
    content_type.eq_ignore_ascii_case("audio/mp4")
        || content_type
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("m4a"))
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ClipMetadata {
    pub tags: Option<String>,
    pub negative_tags: Option<String>,
    pub prompt: Option<String>,
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infill: Option<bool>,
    pub avg_bpm: Option<f64>,
    #[serde(default)]
    pub has_stem: bool,
    #[serde(default)]
    pub is_remix: bool,
    #[serde(default)]
    pub make_instrumental: Option<bool>,
    #[serde(rename = "type")]
    pub clip_type: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ClipActionConfig {
    #[serde(default)]
    pub actions: Vec<ClipAction>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl ClipActionConfig {
    pub fn action(&self, action_type: &str) -> Option<&ClipAction> {
        self.actions
            .iter()
            .find(|action| action.action_type.as_deref() == Some(action_type))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ClipAction {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::Clip;

    #[test]
    fn current_clip_fields_round_trip_without_an_extra_wrapper() {
        let clip: Clip = serde_json::from_value(serde_json::json!({
            "id": "clip-1",
            "title": "Demo",
            "status": "complete",
            "model_name": "chirp-carp",
            "created_at": "2026-07-19T00:00:00Z",
            "is_trashed": false,
            "is_download_unlocked": true,
            "allow_comments": true,
            "action_config": {
                "surface": "song_actions",
                "actions": [{
                    "action_type": "download_song",
                    "disabled": false,
                    "visible": true,
                    "entitlement_reason": "subscribed"
                }]
            },
            "ownership": {"ownership_reason": "subscribed"},
            "media_urls": [{
                "url": "https://cdn.example/demo.mp3",
                "content_type": "mp3",
                "delivery": "progressive"
            }],
            "metadata": {
                "prompt": "[Verse]",
                "duration": 120.0,
                "infill": false,
                "model_badges": {"songcard": {"display_name": "v5.5"}},
                "priority": 10,
                "refund_credits": false,
                "uses_latest_model": true
            }
        }))
        .expect("deserialize current clip response");

        let action = clip
            .action_config
            .as_ref()
            .and_then(|config| config.action("download_song"))
            .expect("typed download action");
        assert_eq!(action.visible, Some(true));
        assert_eq!(action.disabled, Some(false));
        assert_eq!(clip.is_trashed, Some(false));
        assert_eq!(clip.is_download_unlocked, Some(true));
        assert_eq!(clip.metadata.infill, Some(false));

        let output = serde_json::to_value(clip).expect("serialize clip response");
        assert_eq!(output["allow_comments"], true);
        assert_eq!(output["is_download_unlocked"], true);
        assert_eq!(
            output["action_config"]["actions"][0]["action_type"],
            "download_song"
        );
        assert_eq!(output["action_config"]["surface"], "song_actions");
        assert_eq!(
            output["action_config"]["actions"][0]["entitlement_reason"],
            "subscribed"
        );
        assert_eq!(output["ownership"]["ownership_reason"], "subscribed");
        assert_eq!(
            output["media_urls"][0]["url"],
            "https://cdn.example/demo.mp3"
        );
        assert_eq!(
            output["metadata"]["model_badges"]["songcard"]["display_name"],
            "v5.5"
        );
        assert_eq!(output["metadata"]["priority"], 10);
        assert_eq!(output["metadata"]["refund_credits"], false);
        assert_eq!(output["metadata"]["uses_latest_model"], true);
        assert!(output.get("extra").is_none());
        assert!(output["metadata"].get("extra").is_none());
    }

    #[test]
    fn playback_url_prefers_a_usable_top_level_audio_url() {
        let clip: Clip = serde_json::from_value(serde_json::json!({
            "id": "clip-1",
            "title": "Song",
            "status": "complete",
            "model_name": "chirp-fenix",
            "audio_url": "https://cdn.example/clip-1.mp3",
            "created_at": "2026-08-30T00:00:00Z",
            "media_urls": [{
                "url": "https://stream.example/clip-1.m4a",
                "content_type": "m4a-opus",
                "delivery": "progressive"
            }]
        }))
        .expect("clip");

        assert_eq!(clip.playback_url(), Some("https://cdn.example/clip-1.mp3"));
    }

    #[test]
    fn playback_url_uses_progressive_m4a_for_forbidden_legacy_audio_url() {
        let clip: Clip = serde_json::from_value(serde_json::json!({
            "id": "clip-1",
            "title": "Song",
            "status": "complete",
            "model_name": "chirp-fenix",
            "audio_url": "https://studio-api.prod.suno.com/api/forbidden?reason=protected",
            "created_at": "2026-08-30T00:00:00Z",
            "media_urls": [
                {
                    "url": "https://cdn.example/clip-1.mp3",
                    "content_type": "mp3",
                    "delivery": "progressive"
                },
                {
                    "url": "https://stream.example/clip-1.m4a",
                    "content_type": "m4a-opus",
                    "delivery": "progressive"
                }
            ]
        }))
        .expect("clip");

        assert_eq!(
            clip.playback_url(),
            Some("https://stream.example/clip-1.m4a")
        );
    }

    #[test]
    fn playback_url_does_not_treat_protected_mp3_as_a_stream_fallback() {
        let clip: Clip = serde_json::from_value(serde_json::json!({
            "id": "clip-1",
            "title": "Song",
            "status": "complete",
            "model_name": "chirp-fenix",
            "audio_url": "https://studio-api.prod.suno.com/api/forbidden",
            "created_at": "2026-08-30T00:00:00Z",
            "media_urls": [{
                "url": "https://cdn.example/clip-1.mp3",
                "content_type": "mp3",
                "delivery": "progressive"
            }]
        }))
        .expect("clip");

        assert_eq!(clip.playback_url(), None);
    }
}
