use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct DownloadAuthorizationRequest<'a> {
    pub item_id: &'a str,
    pub item_type: &'static str,
}

impl<'a> DownloadAuthorizationRequest<'a> {
    pub(crate) fn clip(clip_id: &'a str) -> Self {
        Self {
            item_id: clip_id,
            item_type: "clip",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DownloadAuthorizationResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credit_deducted: Option<bool>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
