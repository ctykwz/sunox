use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct BillingInfo {
    pub credits: u64,
    pub total_credits_left: u64,
    pub monthly_usage: u64,
    pub monthly_limit: u64,
    pub is_active: bool,
    pub plan: Plan,
    /// Current Web uses a string array here for plan-gated actions. The typed
    /// wrapper also preserves older objects and unknown future shapes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessible_features: Option<AccessibleFeatures>,
    pub models: Vec<Model>,
    pub period: String,
    pub renews_on: Option<String>,
    #[serde(default)]
    pub remaster_model_types: Vec<RemasterModelInfo>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Account-scoped feature gates returned by billing info.
///
/// Suno has returned both a current string array and legacy keyed objects.
/// Keeping the raw value makes account readback forward-compatible while the
/// `contains` helper centralizes fail-closed feature checks.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct AccessibleFeatures(Value);

impl AccessibleFeatures {
    pub fn contains(&self, name: &str) -> bool {
        self.enabled_names().any(|candidate| candidate == name)
    }

    pub fn enabled_names(&self) -> impl Iterator<Item = &str> {
        let mut names = Vec::new();
        match &self.0 {
            Value::Array(features) => {
                for feature in features {
                    match feature {
                        Value::String(name) => names.push(name.as_str()),
                        Value::Object(fields) if feature_object_is_enabled(fields) => {
                            if let Some(name) = fields.get("name").and_then(Value::as_str) {
                                names.push(name);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Value::Object(features) => {
                for (name, enabled) in features {
                    if enabled.as_bool() == Some(true) {
                        names.push(name.as_str());
                    }
                }
            }
            _ => {}
        }
        names.into_iter()
    }
}

fn feature_object_is_enabled(fields: &serde_json::Map<String, Value>) -> bool {
    for flag in [
        "enabled",
        "is_enabled",
        "can_use",
        "accessible",
        "available",
        "is_available",
    ] {
        if let Some(value) = fields.get(flag)
            && value.as_bool() != Some(true)
        {
            return false;
        }
    }
    for flag in ["disabled", "is_disabled"] {
        if let Some(value) = fields.get(flag)
            && value.as_bool() != Some(false)
        {
            return false;
        }
    }
    true
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
    #[serde(default)]
    pub is_default_free_model: bool,
    pub description: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub allowed_condition_combinations: Vec<Vec<String>>,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub badges: Vec<String>,
    #[serde(default)]
    pub max_lengths: MaxLengths,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

impl Model {
    /// Match Suno Web's `modelSupportsFeature`: the `custom` badge enables
    /// Create-form features generally; otherwise the feature must be listed.
    pub fn supports_web_feature(&self, feature: &str) -> bool {
        self.badges.iter().any(|badge| badge == "custom")
            || self.features.iter().any(|candidate| candidate == feature)
    }

    /// Match the current Web model helpers for task capabilities and the
    /// exact active-condition combinations returned by billing info.
    pub fn supports_web_task(&self, task: &str) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability == "all")
            || self
                .capabilities
                .iter()
                .any(|capability| capability == task)
    }

    pub fn supports_web_conditions(&self, conditions: &[&str]) -> bool {
        if self.allowed_condition_combinations.is_empty() {
            return supports_legacy_web_conditions(&self.external_key, conditions);
        }
        self.allowed_condition_combinations.iter().any(|allowed| {
            allowed.len() == conditions.len()
                && conditions
                    .iter()
                    .all(|condition| allowed.iter().any(|item| item == condition))
        })
    }
}

fn supports_legacy_web_conditions(model: &str, conditions: &[&str]) -> bool {
    const BLUEJAY_OR_LATER: &[&str] = &[
        "bluejay",
        "crow",
        "dodo",
        "eagle",
        "fenix",
        "goose",
        "hawk",
        "ibis",
        "chirp-custom",
    ];
    const AUK_OR_LATER: &[&str] = &[
        "auk",
        "bluejay",
        "crow",
        "dodo",
        "eagle",
        "fenix",
        "goose",
        "hawk",
        "ibis",
        "chirp-custom",
    ];
    const V35_OR_LATER: &[&str] = &[
        "v3-5",
        "v4",
        "auk",
        "bluejay",
        "crow",
        "dodo",
        "eagle",
        "fenix",
        "goose",
        "hawk",
        "ibis",
        "chirp-custom",
    ];
    const EXTEND_MODELS: &[&str] = &[
        "v2",
        "v3-0",
        "v3-5",
        "v4",
        "auk",
        "bluejay",
        "crow",
        "dodo",
        "eagle",
        "fenix",
        "goose",
        "hawk",
        "ibis",
        "chirp-custom",
    ];

    let model_matches = |allowed: &[&str]| allowed.iter().any(|part| model.contains(part));
    let contains_all = |required: &[&str]| {
        required
            .iter()
            .all(|required| conditions.contains(required))
    };
    let restrictions: &[(&[&str], &[&str])] = &[
        (EXTEND_MODELS, &["extend"]),
        (V35_OR_LATER, &["persona"]),
        (V35_OR_LATER, &["cover"]),
        (V35_OR_LATER, &["persona", "extend"]),
        (BLUEJAY_OR_LATER, &["playlist"]),
        (BLUEJAY_OR_LATER, &["underpaint"]),
        (BLUEJAY_OR_LATER, &["overpaint"]),
        (AUK_OR_LATER, &["persona", "cover"]),
    ];

    restrictions
        .iter()
        .all(|(allowed_models, required)| !contains_all(required) || model_matches(allowed_models))
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
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
    /// Preserve this legacy account field for raw JSON compatibility. Current
    /// Web lists remaster models without using it as an eligibility gate.
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
                "badges": ["custom"],
                "max_lengths": {"prompt": 5000, "duration": 480},
                "capabilities": ["audio_upload"],
                "allowed_condition_combinations": [["cover"]],
                "major_version": 5
            }],
            "period": "monthly",
            "renews_on": null,
            "accessible_features": {"personas": true},
            "subscription_platform": "stripe"
        }))
        .expect("deserialize current billing response");

        assert!(billing.models[0].supports_web_feature("sound"));
        assert!(
            billing
                .accessible_features
                .as_ref()
                .expect("legacy feature object")
                .contains("personas")
        );
        assert!(
            !billing
                .accessible_features
                .as_ref()
                .expect("legacy feature object")
                .contains("remaster")
        );

        let output = serde_json::to_value(billing).expect("serialize billing response");
        assert_eq!(output["accessible_features"]["personas"], true);
        assert_eq!(output["subscription_platform"], "stripe");
        assert_eq!(output["plan"]["currency"], "USD");
        assert_eq!(output["models"][0]["capabilities"][0], "audio_upload");
        assert_eq!(
            output["models"][0]["allowed_condition_combinations"][0][0],
            "cover"
        );
        assert_eq!(output["models"][0]["features"][0], "reuse_styles_lyrics");
        assert_eq!(output["models"][0]["badges"][0], "custom");
        assert_eq!(output["models"][0]["major_version"], 5);
        assert_eq!(output["models"][0]["max_lengths"]["duration"], 480);
        assert!(output.get("extra").is_none());
    }

    #[test]
    fn accessible_features_support_current_string_arrays_and_preserve_unknown_entries() {
        let billing: BillingInfo = serde_json::from_value(serde_json::json!({
            "credits": 10,
            "total_credits_left": 20,
            "monthly_usage": 1,
            "monthly_limit": 100,
            "is_active": true,
            "plan": {
                "name": "Pro",
                "plan_key": "pro",
                "usage_plan_features": []
            },
            "models": [],
            "period": "monthly",
            "renews_on": null,
            "accessible_features": [
                "remaster",
                {"name": "future_feature", "minimum_tier": "pro"},
                {"name": "enabled_feature", "enabled": true},
                {"name": "disabled_feature", "enabled": false},
                {"name": "blocked_feature", "disabled": true},
                {"name": "malformed_feature", "can_use": "yes"},
                42
            ]
        }))
        .expect("deserialize current feature list");

        let features = billing
            .accessible_features
            .as_ref()
            .expect("accessible features");
        assert!(features.contains("remaster"));
        assert!(features.contains("future_feature"));
        assert!(features.contains("enabled_feature"));
        assert!(!features.contains("disabled_feature"));
        assert!(!features.contains("blocked_feature"));
        assert!(!features.contains("malformed_feature"));
        assert!(!features.contains("missing"));
        assert_eq!(
            features.enabled_names().collect::<Vec<_>>(),
            vec!["remaster", "future_feature", "enabled_feature"]
        );

        let output = serde_json::to_value(billing).expect("serialize feature list");
        assert_eq!(output["accessible_features"][6], 42);
        assert_eq!(output["accessible_features"][1]["minimum_tier"], "pro");
    }

    #[test]
    fn model_feature_support_matches_the_current_web_helper() {
        let model: Model = serde_json::from_value(serde_json::json!({
            "name": "custom",
            "external_key": "custom-model",
            "can_use": true,
            "is_default_model": false,
            "description": "fixture",
            "badges": ["custom"]
        }))
        .expect("deserialize model");

        assert!(model.supports_web_feature("create_control_sliders"));
        assert!(model.supports_web_feature("sound"));
        assert!(!model.supports_web_task("cover"));

        let feature_model: Model = serde_json::from_value(serde_json::json!({
            "name": "feature",
            "external_key": "feature-model",
            "can_use": true,
            "is_default_model": false,
            "description": "fixture",
            "features": ["create_control_sliders"]
        }))
        .expect("deserialize feature model");

        assert!(feature_model.supports_web_feature("create_control_sliders"));
        assert!(!feature_model.supports_web_feature("sound"));

        let capable_model: Model = serde_json::from_value(serde_json::json!({
            "name": "current",
            "external_key": "current-model",
            "can_use": true,
            "is_default_model": true,
            "description": "fixture",
            "capabilities": ["all"],
            "allowed_condition_combinations": [["cover"], ["playlist"]]
        }))
        .expect("deserialize capable model");
        assert!(capable_model.supports_web_task("cover"));
        assert!(capable_model.supports_web_conditions(&["cover"]));
        assert!(!capable_model.supports_web_conditions(&["extend"]));

        let legacy_v3: Model = serde_json::from_value(serde_json::json!({
            "name": "v3",
            "external_key": "chirp-v3-0",
            "can_use": true,
            "is_default_model": false,
            "description": "legacy fallback fixture",
            "capabilities": ["all"]
        }))
        .expect("deserialize legacy model");
        assert!(legacy_v3.supports_web_conditions(&["extend"]));
        assert!(!legacy_v3.supports_web_conditions(&["cover"]));
        assert!(!legacy_v3.supports_web_conditions(&["playlist"]));

        let bluejay: Model = serde_json::from_value(serde_json::json!({
            "name": "v4.5+",
            "external_key": "chirp-bluejay",
            "can_use": true,
            "is_default_model": false,
            "description": "legacy fallback fixture",
            "capabilities": ["all"]
        }))
        .expect("deserialize bluejay model");
        assert!(bluejay.supports_web_conditions(&["cover"]));
        assert!(bluejay.supports_web_conditions(&["playlist"]));
    }

    #[test]
    fn null_allowed_condition_combinations_matches_the_web_empty_fallback() {
        let model: Model = serde_json::from_value(serde_json::json!({
            "name": "v4.5+",
            "external_key": "chirp-bluejay",
            "can_use": true,
            "is_default_model": true,
            "description": "nullable current-field fixture",
            "capabilities": ["all"],
            "allowed_condition_combinations": null
        }))
        .expect("Web treats null allowed conditions as an empty list");

        assert!(model.allowed_condition_combinations.is_empty());
        assert!(model.supports_web_conditions(&["playlist"]));
    }
}
