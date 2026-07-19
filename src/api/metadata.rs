use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use super::types::{
    AlignedWord, ClipReaction, SetClipReactionRequest, SetMetadataRequest, SetVisibilityRequest,
};
use super::{PollingOptions, SunoClient};
use crate::core::{CliError, run_before_deadline, sleep_before_deadline};

const V3_RETRY_DETAIL: &str = "Lyrics alignment not available, try again later.";
const V2_RETRY_DETAIL: &str = "Processing lyrics. Please try again later.";

#[derive(Serialize)]
struct StartAlignedLyricsV3Request<'a> {
    lyrics: &'a str,
    enable_augmentation: bool,
}

#[derive(Deserialize)]
struct AlignedLyricsV3Word {
    word: String,
    start_s: f64,
    end_s: f64,
    #[serde(default)]
    p_align: Option<f64>,
    #[serde(default, flatten)]
    extra: std::collections::BTreeMap<String, serde_json::Value>,
}

impl SunoClient {
    /// Update clip metadata (title, lyrics, caption, cover image).
    pub async fn set_metadata(
        &self,
        clip_id: &str,
        req: &SetMetadataRequest,
    ) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/gen/{clip_id}/set_metadata/"))
                .json(req)
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let text = resp.text().await?;
            if text.trim().is_empty() {
                return Ok(());
            }
            let body: serde_json::Value = serde_json::from_str(&text)?;
            if let Some(error_type) = body.get("error_type").and_then(|value| value.as_str()) {
                let detail = body
                    .get("moderation_error_message")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Suno rejected the clip metadata update")
                    .to_string();
                return Err(CliError::SunoApi {
                    code: "metadata_update_rejected",
                    status: 200,
                    message: format!("{error_type}: {detail}"),
                    retryable: Some(false),
                    details: Some(body),
                });
            }
            Ok(())
        })
        .await
    }

    /// Set clip visibility (public/private).
    pub async fn set_visibility(&self, clip_id: &str, is_public: bool) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/gen/{clip_id}/set_visibility/"))
                .json(&SetVisibilityRequest {
                    is_public,
                    submit_to_contest: false,
                })
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    /// Set or clear a clip like/dislike reaction.
    pub async fn set_clip_reaction(
        &self,
        clip_id: &str,
        reaction: Option<ClipReaction>,
    ) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/gen/{clip_id}/update_reaction_type/"))
                .json(&SetClipReactionRequest::new(reaction))
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    /// Get word-level timestamped lyrics through the current v3 start/poll
    /// workflow. The older v2 read remains only as a compatibility fallback
    /// when v3 is unavailable or the clip has no lyrics payload to submit.
    pub async fn aligned_lyrics(
        &self,
        clip_id: &str,
        lyrics: Option<&str>,
        enable_augmentation: bool,
        polling: PollingOptions,
    ) -> Result<Vec<AlignedWord>, CliError> {
        let deadline = polling.deadline()?;
        if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.is_empty()) {
            match self
                .aligned_lyrics_v3(clip_id, lyrics, enable_augmentation, polling, deadline)
                .await
            {
                Ok(words) => return Ok(words),
                Err(error) if allows_v2_compatibility_fallback(&error) => {}
                Err(error) => return Err(error),
            }
        }
        self.aligned_lyrics_v2(clip_id, polling, deadline).await
    }

    async fn aligned_lyrics_v3(
        &self,
        clip_id: &str,
        lyrics: &str,
        enable_augmentation: bool,
        polling: PollingOptions,
        deadline: Instant,
    ) -> Result<Vec<AlignedWord>, CliError> {
        let path = format!("/api/gen/{clip_id}/aligned_lyrics/v3");
        let req = StartAlignedLyricsV3Request {
            lyrics,
            enable_augmentation,
        };
        let mut body = run_before_deadline(
            deadline,
            self.with_auth_retry(|| async {
                let resp = self.post(&path).json(&req).send().await?;
                let resp = self.check_response(resp).await?;
                Ok(resp.json::<serde_json::Value>().await?)
            }),
            aligned_lyrics_timeout_error(clip_id, polling),
        )
        .await?;
        let mut is_start_response = true;

        loop {
            if let Some(alignment) = body.get("alignment").filter(|value| value.is_array()) {
                return decode_v3_alignment(alignment.clone());
            }
            if body.get("state").and_then(|value| value.as_str()) == Some("error") {
                return Err(CliError::Api {
                    code: "aligned_lyrics_v3",
                    message: body
                        .get("error_message")
                        .and_then(|value| value.as_str())
                        .unwrap_or("Suno v3 lyrics alignment failed")
                        .to_string(),
                });
            }

            let should_retry = body.get("state").and_then(|value| value.as_str())
                == Some("running")
                || body.get("detail").and_then(|value| value.as_str()) == Some(V3_RETRY_DETAIL)
                || is_start_response
                    && body.get("alignment").is_none()
                    && body.get("state").and_then(|value| value.as_str()).is_none();
            if !should_retry {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!("v3 aligned lyrics response had no alignment: {body}"),
                });
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                return Err(aligned_lyrics_timeout_error(clip_id, polling));
            }

            body = run_before_deadline(
                deadline,
                self.with_auth_retry(|| async {
                    let resp = self.get(&path).send().await?;
                    let resp = self.check_response(resp).await?;
                    Ok(resp.json::<serde_json::Value>().await?)
                }),
                aligned_lyrics_timeout_error(clip_id, polling),
            )
            .await?;
            is_start_response = false;
        }
    }

    async fn aligned_lyrics_v2(
        &self,
        clip_id: &str,
        polling: PollingOptions,
        deadline: Instant,
    ) -> Result<Vec<AlignedWord>, CliError> {
        let path = format!("/api/gen/{clip_id}/aligned_lyrics/v2");
        loop {
            let body = run_before_deadline(
                deadline,
                self.with_auth_retry(|| async {
                    let resp = self.get(&path).send().await?;
                    let resp = self.check_response(resp).await?;
                    Ok(resp.json::<serde_json::Value>().await?)
                }),
                aligned_lyrics_timeout_error(clip_id, polling),
            )
            .await?;
            if let Some(words) = body.get("aligned_words").filter(|value| value.is_array()) {
                return Ok(serde_json::from_value(words.clone())?);
            }
            if body.get("detail").and_then(|value| value.as_str()) != Some(V2_RETRY_DETAIL) {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!("v2 aligned lyrics response had no aligned_words: {body}"),
                });
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                return Err(aligned_lyrics_timeout_error(clip_id, polling));
            }
        }
    }
}

fn allows_v2_compatibility_fallback(error: &CliError) -> bool {
    matches!(
        error,
        CliError::Api {
            code: "aligned_lyrics_v3",
            ..
        } | CliError::SunoApi {
            status: 404 | 405 | 501,
            ..
        }
    )
}

fn decode_v3_alignment(value: serde_json::Value) -> Result<Vec<AlignedWord>, CliError> {
    let words: Vec<AlignedLyricsV3Word> = serde_json::from_value(value)?;
    Ok(words
        .into_iter()
        .filter(|word| word.end_s < 6_000.0)
        .map(|word| AlignedWord {
            word: word.word,
            start_s: word.start_s,
            end_s: word.end_s,
            success: true,
            p_align: word.p_align,
            extra: word.extra,
        })
        .collect())
}

fn aligned_lyrics_timeout_error(clip_id: &str, polling: PollingOptions) -> CliError {
    CliError::GenerationFailed(format!(
        "aligned lyrics timed out after {}s for {clip_id}",
        polling.timeout.as_secs()
    ))
}
