use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct CreateCustomModelRequest<'a> {
    pub clip_ids: &'a [String],
    pub name: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CustomModelCreateResponse {
    #[serde(deserialize_with = "deserialize_nonempty_id")]
    pub id: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PendingCustomModelsResponse {
    #[serde(default)]
    pub has_pending: bool,
    #[serde(default)]
    pub pending_models: Vec<PendingCustomModel>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PendingCustomModel {
    #[serde(deserialize_with = "deserialize_nonempty_id")]
    pub id: String,
    pub name: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct ArchiveCustomModelRequest<'a> {
    pub id: &'a str,
}

fn deserialize_nonempty_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let id = String::deserialize(deserializer)?;
    if id.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "Custom Model ID must not be empty",
        ));
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::{CustomModelCreateResponse, PendingCustomModelsResponse};

    #[test]
    fn custom_model_ids_are_required_recovery_handles() {
        for value in [serde_json::json!({}), serde_json::json!({"id": "  "})] {
            serde_json::from_value::<CustomModelCreateResponse>(value)
                .expect_err("create response must contain a usable model ID");
        }

        serde_json::from_value::<PendingCustomModelsResponse>(serde_json::json!({
            "has_pending": true,
            "pending_models": [{"id": "", "name": "Training"}]
        }))
        .expect_err("pending rows must contain usable model IDs");
    }
}
