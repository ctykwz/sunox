use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct CowriteLyricsResponse {
    pub edited_lyrics: String,
    #[serde(default)]
    pub lyrics_request_id: Option<String>,
    #[serde(default)]
    pub lyrics_id: Option<String>,
    #[serde(default)]
    pub variants: Option<Vec<String>>,
    #[serde(default)]
    pub artist_to_tag_mapping: Option<Value>,
    #[serde(default)]
    pub next_prompts: Option<Vec<String>>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AlignedWord {
    pub word: String,
    pub start_s: f64,
    pub end_s: f64,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub p_align: Option<f64>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cowrite_lyrics_unknown_fields_round_trip_at_the_original_level() {
        let result: CowriteLyricsResponse = serde_json::from_value(serde_json::json!({
            "edited_lyrics": "Hello",
            "lyrics_request_id": "request-1",
            "lyrics_id": "lyrics-1",
            "model_name": "new-lyrics-model",
            "safety": {"reviewed": true}
        }))
        .expect("lyrics result");

        let output = serde_json::to_value(result).expect("serialize lyrics result");
        assert_eq!(output["model_name"], "new-lyrics-model");
        assert_eq!(output["safety"]["reviewed"], true);
        assert!(output.get("extra").is_none());
    }
}
