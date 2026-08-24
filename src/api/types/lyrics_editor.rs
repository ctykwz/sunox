use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct LyricsRewriteRequest<'a> {
    pub prompt: &'a str,
    pub context_lyrics_prefix: &'a str,
    pub context_lyrics_edit: &'a str,
    pub context_lyrics_suffix: &'a str,
    pub create_session_token: &'a str,
    pub title: &'a str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LyricsRewriteResponse {
    #[serde(deserialize_with = "deserialize_nonempty_generated_lyrics")]
    pub generated_lyrics: String,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_string")]
    pub lyrics_request_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_string")]
    pub lyrics_id: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Serialize)]
pub struct LyricsRewriteResult {
    pub generated_lyrics: String,
    pub replaced_text: String,
    pub full_text: String,
    pub lyrics_request_id: Option<String>,
    pub lyrics_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl LyricsRewriteResponse {
    /// Match the current Lyrics 2.0 editor's newline-preserving replacement rules.
    pub fn into_editor_result(
        self,
        prefix: &str,
        edited_selection: &str,
        suffix: &str,
    ) -> LyricsRewriteResult {
        let mut replaced_text = self.generated_lyrics.clone();
        if self.generated_lyrics.starts_with('[')
            && !prefix.trim().is_empty()
            && !prefix.ends_with('\n')
        {
            replaced_text.insert(0, '\n');
        }
        if edited_selection.ends_with('\n') && !replaced_text.ends_with('\n') {
            replaced_text.push('\n');
        }
        if edited_selection.is_empty()
            && suffix.trim().is_empty()
            && !prefix.trim().is_empty()
            && !replaced_text.starts_with('\n')
        {
            replaced_text.insert(0, '\n');
        }
        let full_text = format!("{prefix}{replaced_text}{suffix}");
        LyricsRewriteResult {
            generated_lyrics: self.generated_lyrics,
            replaced_text,
            full_text,
            lyrics_request_id: self.lyrics_request_id,
            lyrics_id: self.lyrics_id,
            extra: self.extra,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LyricsMashupRequest<'a> {
    pub lyrics_a: &'a str,
    pub lyrics_b: &'a str,
    pub create_session_token: &'a str,
    pub source: &'static str,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LyricsMashupSubmission {
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_string")]
    pub lyrics_request_id: Option<String>,
    #[serde(deserialize_with = "deserialize_nonempty_mashup_id")]
    pub mashup_id: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LyricsMashupStatus {
    #[serde(deserialize_with = "deserialize_nonempty_status")]
    pub status: String,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub text: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_nonempty_string")]
    pub id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_string")]
    pub error_message: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl LyricsMashupStatus {
    pub fn is_complete(&self) -> bool {
        self.status == "complete"
    }

    pub fn is_failure(&self) -> bool {
        self.status == "error"
    }
}

fn deserialize_nonempty_generated_lyrics<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.is_empty() {
        return Err(serde::de::Error::custom(
            "lyrics rewrite response generated_lyrics must not be empty",
        ));
    }
    Ok(value)
}

fn deserialize_nonempty_mashup_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "lyrics mashup response mashup_id must not be empty",
        ));
    }
    Ok(value)
}

fn deserialize_nonempty_status<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    if value.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "lyrics mashup status must not be empty",
        ));
    }
    Ok(value)
}

fn deserialize_optional_nonempty_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|value| !value.trim().is_empty()))
}

fn deserialize_optional_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::LyricsRewriteResponse;

    #[test]
    fn rewrite_result_matches_current_editor_newline_rules() {
        let response: LyricsRewriteResponse = serde_json::from_value(serde_json::json!({
            "generated_lyrics": "[Chorus]\nnew"
        }))
        .expect("rewrite response");
        let result = response.into_editor_result("[Verse]\nold", "selected\n", "outro");
        assert_eq!(result.replaced_text, "\n[Chorus]\nnew\n");
        assert_eq!(result.full_text, "[Verse]\nold\n[Chorus]\nnew\noutro");
    }
}
