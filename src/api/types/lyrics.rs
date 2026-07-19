use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct LyricsSubmitResponse {
    pub id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LyricsResult {
    pub text: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub error_message: String,
    #[serde(default)]
    pub tags: Vec<String>,
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
    fn lyrics_result_unknown_fields_round_trip_at_the_original_level() {
        let result: LyricsResult = serde_json::from_value(serde_json::json!({
            "text": "Hello",
            "title": "Greeting",
            "status": "complete",
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
