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
    #[serde(default)]
    pub play_count: u64,
    #[serde(default)]
    pub upvote_count: u64,
    #[serde(default)]
    pub metadata: ClipMetadata,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ClipMetadata {
    pub tags: Option<String>,
    pub negative_tags: Option<String>,
    pub prompt: Option<String>,
    pub duration: Option<f64>,
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
            "allow_comments": true,
            "action_config": {
                "actions": [{
                    "action_type": "download_song",
                    "disabled": false,
                    "visible": true
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
                "model_badges": {"songcard": {"display_name": "v5.5"}},
                "priority": 10,
                "refund_credits": false,
                "uses_latest_model": true
            }
        }))
        .expect("deserialize current clip response");

        let output = serde_json::to_value(clip).expect("serialize clip response");
        assert_eq!(output["allow_comments"], true);
        assert_eq!(
            output["action_config"]["actions"][0]["action_type"],
            "download_song"
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
}
