use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LyricsProject {
    #[serde(deserialize_with = "deserialize_nonempty_project_id")]
    pub id: String,
    pub title: String,
    pub lyrics: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LyricsProjectsPage {
    pub projects: Vec<LyricsProject>,
    #[serde(default)]
    pub next_cursor: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct LyricsProjectTitleRequest {
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct FlushLyricsProjectRequest<'a> {
    pub lyrics: &'a str,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FlushLyricsProjectResponse {
    #[serde(deserialize_with = "deserialize_nonempty_updated_at")]
    pub updated_at: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn deserialize_nonempty_project_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "lyrics project ID must not be empty",
        ));
    }
    Ok(value)
}

fn deserialize_nonempty_updated_at<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "lyrics project flush updated_at must not be empty",
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{FlushLyricsProjectResponse, LyricsProject};

    #[test]
    fn lyrics_project_mutation_responses_require_recovery_fields() {
        serde_json::from_value::<LyricsProject>(serde_json::json!({"id": ""}))
            .expect_err("blank project ID must be rejected");
        serde_json::from_value::<FlushLyricsProjectResponse>(serde_json::json!({
            "updated_at": " "
        }))
        .expect_err("blank flush timestamp must be rejected");
    }
}
