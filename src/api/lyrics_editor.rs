use std::time::Duration;

use serde_json::Value;

use super::types::{
    LyricsMashupRequest, LyricsMashupStatus, LyricsMashupSubmission, LyricsRewriteRequest,
    LyricsRewriteResponse,
};
use super::{PollingOptions, SunoClient};
use crate::core::{CliError, MutationAmbiguity, run_before_deadline, sleep_before_deadline};

pub const LYRICS_REWRITE_TIMEOUT: Duration = Duration::from_secs(30);
pub const LYRICS_MASHUP_POLL_INTERVAL: Duration = Duration::from_millis(2_500);

pub struct LyricsRewriteOptions<'a> {
    pub prompt: &'a str,
    pub prefix: &'a str,
    pub edit: &'a str,
    pub suffix: &'a str,
    pub title: &'a str,
    pub create_session_token: &'a str,
}

pub struct LyricsMashupOptions<'a> {
    pub lyrics_a: &'a str,
    pub lyrics_b: &'a str,
    pub create_session_token: &'a str,
}

#[derive(Clone, Copy)]
enum MashupWaitContext {
    Submitted,
    ReadOnly,
}

impl SunoClient {
    /// Rewrite one lyrics selection through the current synchronous Lyrics 2.0 route.
    /// This mutation is intentionally submitted exactly once and is never auth-replayed.
    pub async fn rewrite_lyrics(
        &self,
        options: LyricsRewriteOptions<'_>,
    ) -> Result<LyricsRewriteResponse, CliError> {
        self.rewrite_lyrics_with_timeout(options, LYRICS_REWRITE_TIMEOUT)
            .await
    }

    pub(crate) async fn rewrite_lyrics_with_timeout(
        &self,
        options: LyricsRewriteOptions<'_>,
        timeout: Duration,
    ) -> Result<LyricsRewriteResponse, CliError> {
        validate_rewrite_options(&options)?;
        if timeout.is_zero() {
            return Err(CliError::Config(
                "lyrics rewrite timeout must be greater than zero".into(),
            ));
        }
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = LyricsRewriteRequest {
            prompt: options.prompt,
            context_lyrics_prefix: options.prefix,
            context_lyrics_edit: options.edit,
            context_lyrics_suffix: options.suffix,
            create_session_token: options.create_session_token,
            title: options.title,
        };
        let submit = async {
            let response = self
                .post("/api/generate/lyrics-infill/")
                .json(&request)
                .send()
                .await
                .map_err(|error| {
                    ambiguous_rewrite(
                        &operation_id,
                        "request_send",
                        "http_error",
                        error.to_string(),
                        None,
                        None,
                    )
                })?;
            if response.status().is_server_error() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(ambiguous_rewrite(
                    &operation_id,
                    "response_status",
                    "http_error",
                    format!("HTTP {status}: {body}"),
                    None,
                    None,
                ));
            }
            let response = self
                .check_response(response)
                .await
                .map_err(map_rewrite_api_error)?;
            let raw: Value = response.json().await.map_err(|error| {
                ambiguous_rewrite(
                    &operation_id,
                    "response_body",
                    "http_error",
                    error.to_string(),
                    None,
                    None,
                )
            })?;
            let lyrics_request_id = response_handle(&raw, "lyrics_request_id");
            let lyrics_id = response_handle(&raw, "lyrics_id");
            serde_json::from_value(raw).map_err(|error| {
                ambiguous_rewrite(
                    &operation_id,
                    "response_schema",
                    "schema_drift",
                    error.to_string(),
                    lyrics_request_id.as_deref(),
                    lyrics_id.as_deref(),
                )
            })
        };

        tokio::time::timeout(timeout, submit).await.map_err(|_| {
            ambiguous_rewrite(
                &operation_id,
                "response_timeout",
                "timeout",
                format!(
                    "no reliable response within {} seconds",
                    timeout.as_secs_f64()
                ),
                None,
                None,
            )
        })?
    }

    /// Submit a two-source lyrics mashup exactly once.
    pub async fn start_lyrics_mashup(
        &self,
        options: LyricsMashupOptions<'_>,
    ) -> Result<LyricsMashupSubmission, CliError> {
        validate_mashup_options(&options)?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = LyricsMashupRequest {
            lyrics_a: options.lyrics_a,
            lyrics_b: options.lyrics_b,
            create_session_token: options.create_session_token,
            source: "create_ui",
        };
        let response = self
            .post("/api/generate/lyrics-mashup")
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_mashup_submit(
                    &operation_id,
                    "request_send",
                    "http_error",
                    error.to_string(),
                    None,
                    None,
                )
            })?;
        if response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_mashup_submit(
                &operation_id,
                "response_status",
                "http_error",
                format!("HTTP {status}: {body}"),
                None,
                None,
            ));
        }
        let response = self.check_response(response).await?;
        let raw: Value = response.json().await.map_err(|error| {
            ambiguous_mashup_submit(
                &operation_id,
                "response_body",
                "http_error",
                error.to_string(),
                None,
                None,
            )
        })?;
        let mashup_id = response_handle(&raw, "mashup_id");
        let lyrics_request_id = response_handle(&raw, "lyrics_request_id");
        serde_json::from_value(raw).map_err(|error| {
            ambiguous_mashup_submit(
                &operation_id,
                "response_schema",
                "schema_drift",
                error.to_string(),
                mashup_id.as_deref(),
                lyrics_request_id.as_deref(),
            )
        })
    }

    pub async fn lyrics_mashup_status(
        &self,
        mashup_id: &str,
    ) -> Result<LyricsMashupStatus, CliError> {
        let mashup_id = require_nonempty("lyrics mashup ID", mashup_id)?;
        let path = format!("/api/generate/lyrics/{mashup_id}");
        self.with_auth_retry(|| async {
            let raw: Value = self.read_json_with_transport_retry(self.get(&path)).await?;
            serde_json::from_value(raw).map_err(|error| CliError::Api {
                code: "schema_drift",
                message: format!("invalid lyrics mashup status response: {error}"),
            })
        })
        .await
    }

    pub async fn wait_for_submitted_lyrics_mashup(
        &self,
        mashup_id: &str,
        polling: PollingOptions,
    ) -> Result<LyricsMashupStatus, CliError> {
        self.wait_for_lyrics_mashup_with_context(mashup_id, polling, MashupWaitContext::Submitted)
            .await
    }

    /// Wait for an already-known mashup without claiming that this process submitted it.
    pub async fn wait_for_existing_lyrics_mashup(
        &self,
        mashup_id: &str,
        polling: PollingOptions,
    ) -> Result<LyricsMashupStatus, CliError> {
        self.wait_for_lyrics_mashup_with_context(mashup_id, polling, MashupWaitContext::ReadOnly)
            .await
    }

    async fn wait_for_lyrics_mashup_with_context(
        &self,
        mashup_id: &str,
        polling: PollingOptions,
        context: MashupWaitContext,
    ) -> Result<LyricsMashupStatus, CliError> {
        let mashup_id = require_nonempty("lyrics mashup ID", mashup_id)?;
        let deadline = polling.deadline()?;
        loop {
            let status = run_before_deadline(
                deadline,
                self.lyrics_mashup_status(mashup_id),
                mashup_poll_timeout(mashup_id),
            )
            .await
            .map_err(|error| {
                map_mashup_observation_error(context, mashup_id, "status_poll", error)
            })?;
            if status.is_complete() {
                return Ok(status);
            }
            if status.is_failure() {
                return Err(CliError::GenerationFailed(format!(
                    "lyrics mashup {mashup_id} failed: {}",
                    status
                        .error_message
                        .as_deref()
                        .filter(|message| !message.trim().is_empty())
                        .unwrap_or("Suno returned terminal status error")
                )));
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                return Err(map_mashup_observation_error(
                    context,
                    mashup_id,
                    "status_poll",
                    mashup_poll_timeout(mashup_id),
                ));
            }
        }
    }
}

fn map_mashup_observation_error(
    context: MashupWaitContext,
    mashup_id: &str,
    failed_step: &'static str,
    error: CliError,
) -> CliError {
    match context {
        MashupWaitContext::Submitted => mashup_observation_failure(mashup_id, failed_step, error),
        MashupWaitContext::ReadOnly => error,
    }
}

fn validate_rewrite_options(options: &LyricsRewriteOptions<'_>) -> Result<(), CliError> {
    require_nonempty("lyrics rewrite prompt", options.prompt)?;
    require_nonempty("create session token", options.create_session_token)?;
    Ok(())
}

fn validate_mashup_options(options: &LyricsMashupOptions<'_>) -> Result<(), CliError> {
    require_nonempty("first lyrics source", options.lyrics_a)?;
    require_nonempty("second lyrics source", options.lyrics_b)?;
    require_nonempty("create session token", options.create_session_token)?;
    Ok(())
}

fn require_nonempty<'a>(label: &str, value: &'a str) -> Result<&'a str, CliError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CliError::Config(format!("{label} must not be empty")));
    }
    Ok(value)
}

fn map_rewrite_api_error(error: CliError) -> CliError {
    let lyrics_too_long = matches!(
        &error,
        CliError::SunoApi {
            status: 400,
            details: Some(details),
            ..
        } if details
            .get("detail")
            .and_then(Value::as_str)
            == Some("Lyrics too long to enhance.")
    );
    if lyrics_too_long {
        CliError::Api {
            code: "lyrics_too_long",
            message: "Lyrics too long to enhance.".into(),
        }
    } else {
        error
    }
}

fn response_handle(raw: &Value, key: &str) -> Option<String> {
    raw.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn ambiguous_rewrite(
    operation_id: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
    lyrics_request_id: Option<&str>,
    lyrics_id: Option<&str>,
) -> CliError {
    let mut ambiguity = MutationAmbiguity::new(
        format!(
            "lyrics rewrite operation {operation_id} lost a reliable response during {stage}; Suno may already have generated the replacement"
        ),
        "lyrics_rewrite",
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "the synchronous rewrite route has no safe idempotency key or confirmed status route; do not replay automatically",
        vec!["sunox credits --json".into()],
    );
    if let Some(lyrics_request_id) = lyrics_request_id {
        ambiguity = ambiguity.with_context(
            "lyrics_request_id",
            Value::String(lyrics_request_id.to_string()),
        );
    }
    if let Some(lyrics_id) = lyrics_id {
        ambiguity = ambiguity.with_context("lyrics_id", Value::String(lyrics_id.to_string()));
    }
    ambiguity.into_error()
}

fn ambiguous_mashup_submit(
    operation_id: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
    mashup_id: Option<&str>,
    lyrics_request_id: Option<&str>,
) -> CliError {
    let resumable = mashup_id.is_some();
    let inspection_commands = match mashup_id {
        Some(mashup_id) => vec![
            format!("sunox lyrics mashup-status {mashup_id} --json"),
            format!("sunox lyrics mashup-status {mashup_id} --wait --json"),
        ],
        None => vec!["sunox credits --json".into()],
    };
    let mut ambiguity = MutationAmbiguity::new(
        format!(
            "lyrics mashup operation {operation_id} lost a reliable response during {stage}; Suno may already have accepted the sources"
        ),
        "lyrics_mashup_submit",
        operation_id,
        stage,
        cause_code,
        cause_message,
        resumable,
        if resumable {
            "resume only the read-only poll for the captured mashup ID; never resubmit the sources automatically"
        } else {
            "the submit has no captured idempotency key or mashup ID; do not replay automatically"
        },
        inspection_commands,
    );
    if let Some(mashup_id) = mashup_id {
        ambiguity = ambiguity.with_context("mashup_id", Value::String(mashup_id.to_string()));
    }
    if let Some(lyrics_request_id) = lyrics_request_id {
        ambiguity = ambiguity.with_context(
            "lyrics_request_id",
            Value::String(lyrics_request_id.to_string()),
        );
    }
    ambiguity.into_error()
}

fn mashup_observation_failure(
    mashup_id: &str,
    failed_step: &'static str,
    error: CliError,
) -> CliError {
    CliError::PartialMutation {
        message: format!(
            "lyrics mashup {mashup_id} was submitted but its terminal state could not be confirmed"
        ),
        details: serde_json::json!({
            "operation": "lyrics_mashup",
            "mashup_id": mashup_id,
            "completed_steps": ["mashup_submitted"],
            "failed": {
                "step": failed_step,
                "code": error.error_code(),
                "message": error.to_string(),
            },
            "recovery": {
                "resumable": true,
                "reason": "resume only the read-only status poll; do not resubmit the lyrics sources",
                "inspection_commands": [
                    format!("sunox lyrics mashup-status {mashup_id} --json"),
                    format!("sunox lyrics mashup-status {mashup_id} --wait --json")
                ]
            }
        }),
    }
}

fn mashup_poll_timeout(mashup_id: &str) -> CliError {
    CliError::GenerationFailed(format!("timed out waiting for lyrics mashup {mashup_id}"))
}
