use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct BillingInfo {
    pub credits: u64,
    pub total_credits_left: u64,
    pub monthly_usage: u64,
    pub monthly_limit: u64,
    pub is_active: bool,
    pub plan: Plan,
    pub models: Vec<Model>,
    pub period: String,
    pub renews_on: Option<String>,
    #[serde(default)]
    pub remaster_model_types: Vec<RemasterModelInfo>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Plan {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub plan_key: String,
    #[serde(default)]
    pub usage_plan_features: Vec<Feature>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Feature {
    pub name: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Model {
    pub name: String,
    pub external_key: String,
    pub can_use: bool,
    pub is_default_model: bool,
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub max_lengths: MaxLengths,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Model {
    pub fn supports_capability(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate == "all" || candidate == capability)
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct MaxLengths {
    #[serde(default)]
    pub title: u32,
    #[serde(default)]
    pub prompt: u32,
    #[serde(default)]
    pub tags: u32,
    #[serde(default)]
    pub negative_tags: u32,
    #[serde(default)]
    pub gpt_description_prompt: u32,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemasterModelInfo {
    pub name: String,
    pub external_key: String,
    pub is_default_model: bool,
    /// Suno's billing/info response for remaster models does not include this
    /// field, so keep it optional for deserialization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_use: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::{BillingInfo, Model};

    #[test]
    fn billing_and_model_unknown_fields_round_trip_at_original_level() {
        let billing: BillingInfo = serde_json::from_value(serde_json::json!({
            "credits": 10,
            "total_credits_left": 20,
            "monthly_usage": 1,
            "monthly_limit": 100,
            "is_active": true,
            "plan": {
                "name": "Pro",
                "plan_key": "pro",
                "usage_plan_features": [],
                "currency": "USD"
            },
            "models": [{
                "name": "v5",
                "external_key": "chirp-carp",
                "can_use": true,
                "is_default_model": true,
                "description": "Current model",
                "features": ["reuse_styles_lyrics"],
                "max_lengths": {"prompt": 5000, "duration": 480},
                "capabilities": ["audio_upload"],
                "major_version": 5
            }],
            "period": "monthly",
            "renews_on": null,
            "accessible_features": {"personas": true},
            "subscription_platform": "stripe"
        }))
        .expect("deserialize current billing response");

        assert!(billing.models[0].supports_capability("audio_upload"));
        assert!(!billing.models[0].supports_capability("sound"));

        let output = serde_json::to_value(billing).expect("serialize billing response");
        assert_eq!(output["accessible_features"]["personas"], true);
        assert_eq!(output["subscription_platform"], "stripe");
        assert_eq!(output["plan"]["currency"], "USD");
        assert_eq!(output["models"][0]["capabilities"][0], "audio_upload");
        assert_eq!(output["models"][0]["features"][0], "reuse_styles_lyrics");
        assert_eq!(output["models"][0]["major_version"], 5);
        assert_eq!(output["models"][0]["max_lengths"]["duration"], 480);
        assert!(output.get("extra").is_none());
    }

    #[test]
    fn all_model_capability_matches_web_capability_checks() {
        let model: Model = serde_json::from_value(serde_json::json!({
            "name": "custom",
            "external_key": "custom-model",
            "can_use": true,
            "is_default_model": false,
            "description": "fixture",
            "capabilities": ["all"]
        }))
        .expect("deserialize model");

        assert!(model.supports_capability("create_control_sliders"));
        assert!(model.supports_capability("sound"));
    }
}
