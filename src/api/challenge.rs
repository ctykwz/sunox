use serde::{Deserialize, Serialize};

use super::SunoClient;
use crate::core::CliError;

#[derive(Debug, Deserialize)]
pub struct GenerationChallenge {
    #[serde(default)]
    pub required: bool,
    pub captcha_version: Option<u8>,
}

#[derive(Serialize)]
struct ChallengeCheckRequest<'a> {
    ctype: &'a str,
}

impl SunoClient {
    /// Check the same generation challenge gate the web client calls before
    /// submitting `/api/generate/v2-web/`.
    pub async fn generation_challenge(&self) -> Result<GenerationChallenge, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/c/check")
                .json(&ChallengeCheckRequest {
                    ctype: "generation",
                })
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}
