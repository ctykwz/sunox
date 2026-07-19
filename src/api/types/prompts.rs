use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct PromptUpsampleRequest<'a> {
    pub original_tags: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics: Option<&'a str>,
    pub is_instrumental: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_guidance: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
pub struct PromptUpsampleResponse {
    pub upsampled: String,
    pub request_id: String,
}
