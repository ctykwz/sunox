use std::time::Duration;

use clap::ValueEnum;
use serde::Deserialize;
use tokio::time::Instant;

use super::types::{DownloadAuthorizationRequest, DownloadAuthorizationResponse};
use super::{PollingOptions, SunoClient};
use crate::core::{CliError, MutationAmbiguity, run_before_deadline, sleep_before_deadline};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DownloadFormat {
    Mp3,
    M4a,
    Wav,
    #[value(hide = true)]
    Opus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreparedDownloadFormat {
    Mp3,
    M4a,
    Wav,
    Mp4,
}

impl PreparedDownloadFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Wav => "wav",
            Self::Mp4 => "mp4",
        }
    }
}

impl DownloadFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::M4a => "m4a",
            Self::Wav => "wav",
            Self::Opus => "opus",
        }
    }

    pub fn validate_available(self) -> Result<(), CliError> {
        if self == Self::Opus {
            return Err(CliError::Api {
                code: "unsupported_download_format",
                message: "Suno has retired OPUS conversion (HTTP 410). Choose mp3, m4a, or wav; no download authorization was requested.".into(),
            });
        }
        Ok(())
    }

    pub fn requires_mutation_lock(self) -> bool {
        matches!(self, Self::Wav)
    }
}

#[derive(Deserialize)]
struct PreparedDownload {
    ok: Option<bool>,
    download_url: Option<String>,
    status: Option<PreparedDownloadStatus>,
    reason: Option<String>,
    detail: Option<String>,
    message: Option<String>,
}

#[derive(Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PreparedDownloadStatus {
    Processing,
    Ready,
    Error,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WavFile {
    File {
        #[serde(deserialize_with = "deserialize_existing_download_url")]
        wav_file_url: Option<String>,
    },
    Empty(EmptyWavFile),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyWavFile {}

impl WavFile {
    fn url(self) -> Option<String> {
        match self {
            Self::File { wav_file_url } => wav_file_url,
            Self::Empty(_) => None,
        }
    }
}

// Null and the observed exact empty WAV object mean no file exists.
// Unknown nonempty objects must never authorize a conversion.
fn deserialize_existing_download_url<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let url = Option::<String>::deserialize(deserializer)?;
    if url.as_ref().is_some_and(|url| url.trim().is_empty()) {
        return Err(serde::de::Error::custom(
            "download URL must be nonblank or null",
        ));
    }
    Ok(url)
}

impl SunoClient {
    pub async fn authorize_download(
        &self,
        clip_id: &str,
    ) -> Result<DownloadAuthorizationResponse, CliError> {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = self
            .post_without_redirect("/api/download/authorize")
            .json(&DownloadAuthorizationRequest::clip(clip_id));
        let response = self
            .prepare_mutation_request(request)
            .await?
            .send()
            .await
            .map_err(|error| {
                ambiguous_download_authorization_details(
                    &operation_id,
                    clip_id,
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        let status = response.status();
        let response = match self.check_response(response).await {
            Ok(response) => response,
            Err(error) if status.is_server_error() || status.is_redirection() => {
                return Err(ambiguous_download_authorization_from_cli_error(
                    &operation_id,
                    clip_id,
                    "response_status",
                    error,
                ));
            }
            Err(error) => return Err(error),
        };
        let body = response.bytes().await.map_err(|error| {
            ambiguous_download_authorization_details(
                &operation_id,
                clip_id,
                "response_body",
                "http_error",
                error.to_string(),
            )
        })?;
        let response_value: serde_json::Value = serde_json::from_slice(&body).map_err(|error| {
            ambiguous_download_authorization_details(
                &operation_id,
                clip_id,
                "response_body",
                "json_error",
                error.to_string(),
            )
        })?;
        crate::core::operation::record_response("/api/download/authorize", &response_value)
            .map_err(|error| {
                ambiguous_download_authorization_from_cli_error(
                    &operation_id,
                    clip_id,
                    "checkpoint_persist",
                    error,
                )
            })?;
        let authorization: DownloadAuthorizationResponse = serde_json::from_value(response_value)
            .map_err(|error| {
            ambiguous_download_authorization_details(
                &operation_id,
                clip_id,
                "response_schema",
                "schema_drift",
                error.to_string(),
            )
        })?;
        if authorization.ok.is_none() {
            return Err(ambiguous_download_authorization_details(
                &operation_id,
                clip_id,
                "response_schema",
                "schema_drift",
                "download authorization response omitted required field `ok`".into(),
            ));
        }
        Ok(authorization)
    }

    pub async fn download_url(
        &self,
        clip_id: &str,
        format: DownloadFormat,
        polling: PollingOptions,
    ) -> Result<String, CliError> {
        self.download_url_with_conversion_policy(clip_id, format, polling, true)
            .await
    }

    pub(crate) async fn prepared_download_url(
        &self,
        clip_id: &str,
        format: PreparedDownloadFormat,
        polling: PollingOptions,
    ) -> Result<String, CliError> {
        let deadline = polling.deadline()?;
        self.poll_prepared_download_url(clip_id, format.extension(), deadline, polling.interval)
            .await
    }

    pub async fn download_url_with_conversion_policy(
        &self,
        clip_id: &str,
        format: DownloadFormat,
        polling: PollingOptions,
        allow_conversion: bool,
    ) -> Result<String, CliError> {
        format.validate_available()?;
        let deadline = polling.deadline()?;
        match format {
            DownloadFormat::Mp3 | DownloadFormat::M4a => {
                self.poll_prepared_download_url(
                    clip_id,
                    format.extension(),
                    deadline,
                    polling.interval,
                )
                .await
            }
            DownloadFormat::Wav => {
                self.generated_or_existing_wav_url(
                    clip_id,
                    deadline,
                    polling.interval,
                    allow_conversion,
                )
                .await
            }
            DownloadFormat::Opus => unreachable!("retired format rejected before requests"),
        }
    }

    async fn wav_url_if_ready(&self, clip_id: &str) -> Result<Option<String>, CliError> {
        Ok(self.wav_file(clip_id).await?.url())
    }

    async fn poll_prepared_download_url(
        &self,
        clip_id: &str,
        format: &str,
        deadline: Instant,
        poll_interval: Duration,
    ) -> Result<String, CliError> {
        loop {
            let path = format!("/api/download/clip/{clip_id}?format={format}");
            let prepared: PreparedDownload = run_before_deadline(
                deadline,
                self.with_auth_retry(|| async {
                    let resp = self.get(&path).send().await?;
                    let resp = self.check_response(resp).await?;
                    Ok(resp.json().await?)
                }),
                download_timeout(format, clip_id),
            )
            .await?;
            if prepared.reason.as_deref() != Some("rate_limited") {
                if prepared.ok != Some(true)
                    || prepared.status == Some(PreparedDownloadStatus::Error)
                {
                    return Err(CliError::Download(
                        prepared
                            .detail
                            .or(prepared.message)
                            .unwrap_or_else(|| "Suno download preparation failed".into()),
                    ));
                }
                match prepared.status {
                    Some(PreparedDownloadStatus::Ready) => {
                        return prepared
                            .download_url
                            .filter(|url| !url.trim().is_empty())
                            .ok_or_else(|| CliError::Api {
                                code: "schema_drift",
                                message: "ready download response omitted a usable URL".into(),
                            });
                    }
                    Some(PreparedDownloadStatus::Processing) => {}
                    _ => {
                        return Err(CliError::Api {
                            code: "schema_drift",
                            message: "prepared download omitted a recognized status".into(),
                        });
                    }
                }
            }
            if !sleep_before_deadline(deadline, poll_interval).await {
                return Err(CliError::Download(format!(
                    "timed out waiting for {format} download URL for clip {clip_id}"
                )));
            }
        }
    }

    async fn generated_wav_url(
        &self,
        clip_id: &str,
        deadline: Instant,
        poll_interval: Duration,
    ) -> Result<String, CliError> {
        let path = format!("/api/gen/{clip_id}/convert_wav/");
        let operation_id = uuid::Uuid::new_v4().to_string();
        run_before_deadline(
            deadline,
            async {
                let request = self.post_without_redirect(&path);
                let resp = self
                    .prepare_mutation_request(request)
                    .await?
                    .send()
                    .await
                    .map_err(|error| {
                        ambiguous_conversion(&operation_id, clip_id, "wav", "request_send", error)
                    })?;
                if resp.status().is_redirection() || resp.status().is_server_error() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ambiguous_conversion_details(
                        &operation_id,
                        clip_id,
                        "wav",
                        "response_status",
                        "http_error",
                        format!("HTTP {status}: {body}"),
                    ));
                }
                self.check_response(resp).await?;
                Ok(())
            },
            download_timeout("WAV file", clip_id),
        )
        .await
        .map_err(|error| {
            ambiguous_conversion_submit_outcome(&operation_id, clip_id, "wav", "submit_wait", error)
        })?;

        loop {
            let file = run_before_deadline(
                deadline,
                self.wav_file(clip_id),
                download_timeout("WAV file", clip_id),
            )
            .await
            .map_err(|error| {
                ambiguous_conversion_from_cli_error(
                    &operation_id,
                    clip_id,
                    "wav",
                    "file_poll",
                    error,
                )
            })?;
            if let Some(url) = file.url() {
                return Ok(url);
            }
            if !sleep_before_deadline(deadline, poll_interval).await {
                return Err(ambiguous_conversion_from_cli_error(
                    &operation_id,
                    clip_id,
                    "wav",
                    "file_poll",
                    download_timeout("WAV file", clip_id),
                ));
            }
        }
    }

    async fn generated_or_existing_wav_url(
        &self,
        clip_id: &str,
        deadline: Instant,
        poll_interval: Duration,
        allow_conversion: bool,
    ) -> Result<String, CliError> {
        let existing = run_before_deadline(
            deadline,
            self.wav_url_if_ready(clip_id),
            download_timeout("WAV file", clip_id),
        )
        .await?;
        if let Some(url) = existing {
            return Ok(url);
        }
        if !allow_conversion {
            return Err(conversion_disabled("WAV", clip_id));
        }
        self.generated_wav_url(clip_id, deadline, poll_interval)
            .await
    }

    async fn wav_file(&self, clip_id: &str) -> Result<WavFile, CliError> {
        let path = format!("/api/gen/{clip_id}/wav_file/");
        self.with_auth_retry(|| async {
            let resp = self.get(&path).send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}

fn conversion_disabled(format: &str, clip_id: &str) -> CliError {
    CliError::Download(format!(
        "no existing {format} file is available for clip {clip_id}; --no-convert/read-only mode refused to start server-side conversion"
    ))
}

fn ambiguous_conversion_submit_outcome(
    operation_id: &str,
    clip_id: &str,
    format: &'static str,
    stage: &'static str,
    error: CliError,
) -> CliError {
    if !matches!(error, CliError::Download(_)) {
        return error;
    }
    ambiguous_conversion_from_cli_error(operation_id, clip_id, format, stage, error)
}

fn ambiguous_conversion(
    operation_id: &str,
    clip_id: &str,
    format: &'static str,
    stage: &'static str,
    error: reqwest::Error,
) -> CliError {
    ambiguous_conversion_details(
        operation_id,
        clip_id,
        format,
        stage,
        "http_error",
        error.to_string(),
    )
}

fn ambiguous_conversion_from_cli_error(
    operation_id: &str,
    clip_id: &str,
    format: &'static str,
    stage: &'static str,
    error: CliError,
) -> CliError {
    let code = error.error_code();
    let message = error.to_string();
    ambiguous_conversion_details(operation_id, clip_id, format, stage, code, message)
}

fn ambiguous_conversion_details(
    operation_id: &str,
    clip_id: &str,
    format: &'static str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "{format} conversion operation {operation_id} did not reach a reliable terminal result during {stage}; conversion may still complete"
        ),
        format!("convert_{format}"),
        operation_id,
        stage,
        cause_code,
        cause_message,
        true,
        "inspect the existing converted-file URL; do not start conversion again while the outcome is unknown",
        vec![format!(
            "sunox clip download {clip_id} --format {format} --no-convert --json"
        )],
    )
    .with_context(
        "clip_id",
        serde_json::Value::String(clip_id.to_string()),
    )
    .with_context("format", serde_json::Value::String(format.to_string()))
    .into_error()
}

fn download_timeout(format: &str, clip_id: &str) -> CliError {
    CliError::Download(format!(
        "timed out waiting for {format} download URL for clip {clip_id}"
    ))
}

fn ambiguous_download_authorization_from_cli_error(
    operation_id: &str,
    clip_id: &str,
    stage: &'static str,
    error: CliError,
) -> CliError {
    let cause_code = error.error_code();
    let cause_message = error.to_string();
    ambiguous_download_authorization_details(
        operation_id,
        clip_id,
        stage,
        cause_code,
        cause_message,
    )
}

fn ambiguous_download_authorization_details(
    operation_id: &str,
    clip_id: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "download authorization {operation_id} for clip {clip_id} lost a reliable response during {stage}; the clip may already be unlocked or a download credit may have been deducted"
        ),
        "download_authorize",
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "read back the clip unlock state and billing usage before deciding whether another authorization is safe",
        vec![
            format!("sunox clip info {clip_id} --json"),
            "sunox credits --json".into(),
        ],
    )
    .with_context(
        "clip_id",
        serde_json::Value::String(clip_id.to_string()),
    )
    .into_error()
}
