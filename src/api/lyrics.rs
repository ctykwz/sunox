use serde_json::json;

use super::PollingOptions;
use super::SunoClient;
use super::types::{LyricsResult, LyricsSubmitResponse};
use crate::core::{CliError, run_before_deadline, sleep_before_deadline};

impl SunoClient {
    /// Submit lyrics generation and poll until complete.
    pub async fn generate_lyrics(&self, prompt: &str) -> Result<LyricsResult, CliError> {
        self.generate_lyrics_with_polling(
            prompt,
            PollingOptions {
                timeout: std::time::Duration::from_secs(60),
                interval: std::time::Duration::from_secs(2),
            },
        )
        .await
    }

    pub(crate) async fn generate_lyrics_with_polling(
        &self,
        prompt: &str,
        polling: PollingOptions,
    ) -> Result<LyricsResult, CliError> {
        polling.validate()?;
        let submit: LyricsSubmitResponse = self
            .with_auth_retry(|| async {
                let resp = self
                    .post("/api/generate/lyrics/")
                    .json(&json!({ "prompt": prompt }))
                    .send()
                    .await?;
                let resp = self.check_response(resp).await?;
                Ok(resp.json().await?)
            })
            .await?;

        let deadline = polling.deadline()?;
        let mut delay = polling.interval;

        loop {
            if !sleep_before_deadline(deadline, delay).await {
                return Err(lyrics_timeout());
            }

            let path = format!("/api/generate/lyrics/{}", submit.id);
            let result: LyricsResult = run_before_deadline(
                deadline,
                self.with_auth_retry(|| async {
                    let resp = self.get(&path).send().await?;
                    let resp = self.check_response(resp).await?;
                    Ok(resp.json().await?)
                }),
                lyrics_timeout(),
            )
            .await?;

            if !result.error_message.is_empty() {
                return Err(CliError::GenerationFailed(result.error_message));
            }
            if result.status == "complete" {
                return Ok(result);
            }
            delay = (delay * 2).min(std::time::Duration::from_secs(8));
        }
    }
}

fn lyrics_timeout() -> CliError {
    CliError::GenerationFailed("lyrics generation timed out".into())
}
