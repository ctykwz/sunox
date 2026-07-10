use std::time::Duration;

use clap::ValueEnum;
use serde::Deserialize;
use tokio::time::Instant;

use super::{PollingOptions, SunoClient};
use crate::core::{CliError, run_before_deadline, sleep_before_deadline};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum DownloadFormat {
    Mp3,
    M4a,
    Wav,
    Opus,
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

    pub fn requires_mutation_lock(self) -> bool {
        matches!(self, Self::Wav | Self::Opus)
    }
}

#[derive(Deserialize)]
struct PreparedDownload {
    download_url: Option<String>,
    status: Option<String>,
}

#[derive(Deserialize)]
struct WavFile {
    wav_file_url: Option<String>,
}

#[derive(Deserialize)]
struct OpusFile {
    opus_file_url: Option<String>,
}

impl SunoClient {
    pub async fn download_url(
        &self,
        clip_id: &str,
        format: DownloadFormat,
        polling: PollingOptions,
    ) -> Result<String, CliError> {
        let deadline = polling.deadline()?;
        match format {
            DownloadFormat::Mp3 | DownloadFormat::M4a => {
                self.prepared_download_url(clip_id, format.extension(), deadline, polling.interval)
                    .await
            }
            DownloadFormat::Wav => {
                self.generated_wav_url(clip_id, deadline, polling.interval)
                    .await
            }
            DownloadFormat::Opus => {
                self.generated_or_existing_opus_url(clip_id, deadline, polling.interval)
                    .await
            }
        }
    }

    async fn opus_url_if_ready(&self, clip_id: &str) -> Result<Option<String>, CliError> {
        Ok(self.opus_file(clip_id).await?.opus_file_url)
    }

    async fn prepared_download_url(
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
            if let Some(url) = prepared.download_url {
                return Ok(url);
            }
            if prepared.status.as_deref() != Some("processing") {
                return Err(CliError::Download(format!(
                    "no {format} download URL available for clip {clip_id}"
                )));
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
        run_before_deadline(
            deadline,
            self.with_auth_retry(|| async {
                let resp = self.post(&path).send().await?;
                self.check_response(resp).await?;
                Ok(())
            }),
            download_timeout("WAV file", clip_id),
        )
        .await?;

        loop {
            let file = run_before_deadline(
                deadline,
                self.wav_file(clip_id),
                download_timeout("WAV file", clip_id),
            )
            .await?;
            if let Some(url) = file.wav_file_url {
                return Ok(url);
            }
            if !sleep_before_deadline(deadline, poll_interval).await {
                return Err(CliError::Download(format!(
                    "timed out waiting for WAV file for clip {clip_id}"
                )));
            }
        }
    }

    async fn generated_opus_url(
        &self,
        clip_id: &str,
        deadline: Instant,
        poll_interval: Duration,
    ) -> Result<String, CliError> {
        let path = format!("/api/gen/{clip_id}/convert_opus");
        run_before_deadline(
            deadline,
            self.with_auth_retry(|| async {
                let resp = self.post(&path).send().await?;
                self.check_response(resp).await?;
                Ok(())
            }),
            download_timeout("OPUS file", clip_id),
        )
        .await?;

        loop {
            let file = run_before_deadline(
                deadline,
                self.opus_file(clip_id),
                download_timeout("OPUS file", clip_id),
            )
            .await?;
            if let Some(url) = file.opus_file_url {
                return Ok(url);
            }
            if !sleep_before_deadline(deadline, poll_interval).await {
                return Err(CliError::Download(format!(
                    "timed out waiting for OPUS file for clip {clip_id}"
                )));
            }
        }
    }

    async fn generated_or_existing_opus_url(
        &self,
        clip_id: &str,
        deadline: Instant,
        poll_interval: Duration,
    ) -> Result<String, CliError> {
        let existing = run_before_deadline(
            deadline,
            self.opus_url_if_ready(clip_id),
            download_timeout("OPUS file", clip_id),
        )
        .await?;
        if let Some(url) = existing {
            return Ok(url);
        }
        self.generated_opus_url(clip_id, deadline, poll_interval)
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

    async fn opus_file(&self, clip_id: &str) -> Result<OpusFile, CliError> {
        let path = format!("/api/gen/{clip_id}/opus_file/");
        self.with_auth_retry(|| async {
            let resp = self.get(&path).send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}

fn download_timeout(format: &str, clip_id: &str) -> CliError {
    CliError::Download(format!(
        "timed out waiting for {format} download URL for clip {clip_id}"
    ))
}
