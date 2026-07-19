use std::path::Path;
use std::time::Duration;

use crate::api::SunoClient;
use crate::api::types::{
    Clip, CreateAudioUploadRequest, CreateAudioUploadSpec, FinishAudioUploadRequest,
    InitializeAudioClipRequest, SetMetadataRequest,
};
use crate::core::{
    CliError, deadline_after, ensure_poll_timeout, run_before_deadline, sleep_before_deadline,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct UploadResult {
    pub upload_id: String,
    pub status_upload_id: Option<String>,
    pub clip_id: Option<String>,
    pub clip: Option<Clip>,
    pub has_vocal: Option<bool>,
    pub status: String,
}

pub struct UploadWorkflowInput<'a> {
    pub file: &'a Path,
    pub upload_type: &'a str,
    pub is_stem_mix: bool,
    pub title: Option<String>,
    pub lyrics: Option<String>,
    pub timeout: Duration,
    pub poll_interval: Duration,
}

pub async fn run(
    client: &SunoClient,
    input: UploadWorkflowInput<'_>,
) -> Result<UploadResult, CliError> {
    ensure_poll_timeout(input.timeout)?;
    let extension = audio_extension(input.file)?;
    let filename = upload_filename(input.file)?;
    let file = tokio::fs::File::open(input.file).await?;
    let metadata = file.metadata().await?;
    if !metadata.is_file() {
        return Err(CliError::Config(format!(
            "upload path is not a regular file: {}",
            input.file.display()
        )));
    }

    let upload = client
        .create_audio_upload(&CreateAudioUploadRequest {
            spec: CreateAudioUploadSpec {
                extension,
                is_stem_mix: input.is_stem_mix,
                upload_type: input.upload_type.to_string(),
            },
        })
        .await?;
    let mut completed_steps = vec!["upload_created"];

    client
        .upload_presigned_audio_file(&upload.url, &upload.fields, &filename, file, metadata.len())
        .await
        .map_err(|error| {
            upload_stage_error(&upload.id, None, &completed_steps, "file_upload", error)
        })?;
    completed_steps.push("file_uploaded");

    client
        .finish_audio_upload(
            &upload.id,
            &FinishAudioUploadRequest {
                upload_type: input.upload_type.to_string(),
                upload_filename: filename,
                agreed_to_vip_upload_terms: false,
            },
        )
        .await
        .map_err(|error| {
            upload_stage_error(&upload.id, None, &completed_steps, "upload_finish", error)
        })?;
    completed_steps.push("upload_finished");

    let status = wait_until_complete(client, &upload.id, input.timeout, input.poll_interval)
        .await
        .map_err(|error| {
            upload_stage_error(&upload.id, None, &completed_steps, "processing_wait", error)
        })?;
    completed_steps.push("processing_complete");
    let initialized = client
        .initialize_audio_clip(
            &upload.id,
            &InitializeAudioClipRequest {
                downbeats: None,
                user_reviewed_tags: Some(true),
            },
        )
        .await
        .map_err(|error| {
            upload_stage_error(&upload.id, None, &completed_steps, "clip_initialize", error)
        })?;
    completed_steps.push("clip_initialized");

    let clip_id = initialized_clip_id(&initialized).map_err(|error| {
        upload_stage_error(&upload.id, None, &completed_steps, "clip_identity", error)
    })?;

    let title = input.title.or_else(|| status.title.clone());
    let lyrics = input.lyrics;
    let image_url = status.image_url.clone();
    let expected_metadata = ExpectedClipMetadata {
        title: title.clone(),
        lyrics: lyrics.clone(),
        image_url: image_url.clone(),
    };
    let updates_metadata = title.is_some() || lyrics.is_some() || image_url.is_some();
    if updates_metadata {
        client
            .set_metadata(
                &clip_id,
                &SetMetadataRequest {
                    title,
                    lyrics,
                    caption: None,
                    image_url,
                    image_s3_id: None,
                    is_audio_upload_tos_accepted: Some(true),
                    remove_image_cover: None,
                    remove_video_cover: None,
                },
            )
            .await
            .map_err(|error| {
                upload_metadata_stage_error(
                    &upload.id,
                    &clip_id,
                    &completed_steps,
                    &expected_metadata,
                    error,
                )
            })?;
        completed_steps.push("metadata_updated");
    }

    let clip = if updates_metadata {
        Some(
            wait_for_metadata_readback(
                client,
                &clip_id,
                &expected_metadata,
                input.timeout,
                input.poll_interval,
            )
            .await
            .map_err(|error| {
                upload_stage_error(
                    &upload.id,
                    Some(&clip_id),
                    &completed_steps,
                    "readback",
                    error,
                )
            })?,
        )
    } else {
        initialized.clip
    };

    Ok(UploadResult {
        upload_id: upload.id,
        status_upload_id: status.id,
        clip_id: Some(clip_id),
        clip,
        has_vocal: status.has_vocal,
        status: "complete".into(),
    })
}

struct ExpectedClipMetadata {
    title: Option<String>,
    lyrics: Option<String>,
    image_url: Option<String>,
}

impl ExpectedClipMetadata {
    fn matches(&self, clip: &Clip) -> bool {
        self.title.as_ref().is_none_or(|title| clip.title == *title)
            && self
                .lyrics
                .as_ref()
                .is_none_or(|lyrics| clip.metadata.prompt.as_ref() == Some(lyrics))
            && self
                .image_url
                .as_ref()
                .is_none_or(|image_url| clip.image_url.as_ref() == Some(image_url))
    }
}

async fn wait_for_metadata_readback(
    client: &SunoClient,
    clip_id: &str,
    expected: &ExpectedClipMetadata,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<Clip, CliError> {
    let deadline = deadline_after(timeout)?;
    let poll_interval = poll_interval.max(Duration::from_millis(1));
    let requested = [clip_id.to_string()];
    loop {
        let clips = run_before_deadline(
            deadline,
            client.get_clips(&requested),
            metadata_readback_timeout(clip_id, timeout),
        )
        .await?;
        if let Some(clip) = clips.into_iter().find(|clip| clip.id == clip_id)
            && expected.matches(&clip)
        {
            return Ok(clip);
        }
        if !sleep_before_deadline(deadline, poll_interval).await {
            return Err(metadata_readback_timeout(clip_id, timeout));
        }
    }
}

fn metadata_readback_timeout(clip_id: &str, timeout: Duration) -> CliError {
    CliError::GenerationFailed(format!(
        "audio upload metadata for clip {clip_id} did not become visible within {} seconds",
        timeout.as_secs()
    ))
}

fn upload_stage_error(
    upload_id: &str,
    clip_id: Option<&str>,
    completed_steps: &[&str],
    failed_step: &str,
    error: CliError,
) -> CliError {
    let mut details = serde_json::json!({
        "operation": "audio_upload",
        "upload_id": upload_id,
        "completed_steps": completed_steps,
        "failed": {
            "step": failed_step,
            "code": error.error_code(),
            "message": error.to_string()
        }
    });
    if let Some(clip_id) = clip_id {
        details["clip_id"] = serde_json::Value::String(clip_id.to_string());
    }
    details["recovery"] = upload_recovery_details(upload_id, clip_id, failed_step);
    CliError::PartialMutation {
        message: format!(
            "audio upload {upload_id} stopped at {failed_step} after {} completed step(s)",
            completed_steps.len()
        ),
        details,
    }
}

fn upload_metadata_stage_error(
    upload_id: &str,
    clip_id: &str,
    completed_steps: &[&str],
    expected: &ExpectedClipMetadata,
    error: CliError,
) -> CliError {
    let mut error = upload_stage_error(
        upload_id,
        Some(clip_id),
        completed_steps,
        "metadata_update",
        error,
    );
    if let CliError::PartialMutation { details, .. } = &mut error {
        let mut arguments = serde_json::json!({ "clip_id": clip_id });
        if let Some(title) = &expected.title {
            arguments["title"] = serde_json::Value::String(title.clone());
        }
        if let Some(lyrics) = &expected.lyrics {
            arguments["lyrics"] = serde_json::Value::String(lyrics.clone());
        }
        if let Some(image_url) = &expected.image_url {
            arguments["image_url"] = serde_json::Value::String(image_url.clone());
        }
        details["recovery"] = serde_json::json!({
            "resumable": true,
            "command": "sunox clip set",
            "arguments": arguments
        });
    }
    error
}

fn upload_recovery_details(
    upload_id: &str,
    clip_id: Option<&str>,
    failed_step: &str,
) -> serde_json::Value {
    match (failed_step, clip_id) {
        ("processing_wait", _) => serde_json::json!({
            "resumable": false,
            "reason": "clip initialization cannot be safely replayed",
            "inspection": {
                "command": "sunox clip upload-status",
                "arguments": { "upload_id": upload_id }
            }
        }),
        ("metadata_update", _) => serde_json::json!({
            "resumable": false,
            "reason": "metadata recovery requires the exact requested fields"
        }),
        ("readback", Some(clip_id)) => serde_json::json!({
            "resumable": true,
            "command": "sunox clip info",
            "arguments": { "clip_id": clip_id }
        }),
        ("file_upload", _) => serde_json::json!({
            "resumable": false,
            "reason": "the presigned upload form cannot be safely reconstructed"
        }),
        ("upload_finish", _) => serde_json::json!({
            "resumable": false,
            "reason": "retry safety for audio upload finish is not live-verified"
        }),
        ("clip_initialize" | "clip_identity", _) => serde_json::json!({
            "resumable": false,
            "reason": "retry safety for clip initialization is not live-verified"
        }),
        _ => serde_json::json!({ "resumable": false }),
    }
}

pub fn audio_extension(path: &Path) -> Result<String, CliError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .filter(|extension| !extension.is_empty())
        .ok_or_else(|| CliError::Config("upload file must have an audio extension".into()))?;
    Ok(extension)
}

pub fn upload_filename(path: &Path) -> Result<String, CliError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| CliError::Config("upload file must have a valid filename".into()))
}

fn initialized_clip_id(
    initialized: &crate::api::types::InitializeAudioClipResponse,
) -> Result<String, CliError> {
    initialized
        .clip_id
        .clone()
        .or_else(|| initialized.clip.as_ref().map(|clip| clip.id.clone()))
        .ok_or_else(|| CliError::Api {
            code: "schema_drift",
            message: "audio upload initialization completed without a clip id".into(),
        })
}

async fn wait_until_complete(
    client: &SunoClient,
    upload_id: &str,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<crate::api::types::AudioUploadStatus, CliError> {
    let deadline = deadline_after(timeout)?;
    let poll_interval = poll_interval.max(Duration::from_secs(1));
    loop {
        let timeout_error = || {
            CliError::GenerationFailed(format!(
                "audio upload {upload_id} did not complete within {} seconds",
                timeout.as_secs()
            ))
        };
        let status = run_before_deadline(
            deadline,
            client.get_audio_upload(upload_id),
            timeout_error(),
        )
        .await?;
        match status.status.as_deref() {
            Some("complete") => return Ok(status),
            Some("error") => {
                return Err(CliError::GenerationFailed(format!(
                    "audio upload {upload_id} failed during processing"
                )));
            }
            _ if !sleep_before_deadline(deadline, poll_interval).await => {
                return Err(timeout_error());
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    use crate::api::SunoClient;
    use crate::api::types::InitializeAudioClipResponse;
    use crate::auth::AuthState;
    use crate::core::CliError;

    use super::{
        ExpectedClipMetadata, UploadWorkflowInput, audio_extension, initialized_clip_id, run,
        upload_filename, upload_metadata_stage_error, upload_stage_error, wait_until_complete,
    };

    #[test]
    fn audio_extension_lowercases_file_extension() {
        assert_eq!(
            audio_extension(Path::new("/tmp/Demo.MP3")).expect("extension"),
            "mp3"
        );
    }

    #[test]
    fn upload_filename_uses_basename() {
        assert_eq!(
            upload_filename(Path::new("/tmp/Demo.MP3")).expect("filename"),
            "Demo.MP3"
        );
    }

    #[test]
    fn initialized_clip_id_rejects_missing_clip_identity() {
        let response = InitializeAudioClipResponse {
            clip_id: None,
            clip: None,
        };

        let err = initialized_clip_id(&response).expect_err("missing clip id");

        assert_eq!(err.error_code(), "schema_drift");
    }

    #[test]
    fn upload_stage_failure_exposes_recovery_checkpoint() {
        let error = upload_metadata_stage_error(
            "upload-1",
            "clip-1",
            &["upload_created", "file_uploaded", "processing_complete"],
            &ExpectedClipMetadata {
                title: Some("Final title".into()),
                lyrics: Some("Final lyrics".into()),
                image_url: Some("https://cdn.example/cover.jpeg".into()),
            },
            CliError::RateLimited,
        );

        assert_eq!(error.error_code(), "partial_mutation");
        assert_eq!(
            error.details().expect("upload checkpoint details"),
            &serde_json::json!({
                "operation": "audio_upload",
                "upload_id": "upload-1",
                "clip_id": "clip-1",
                "completed_steps": ["upload_created", "file_uploaded", "processing_complete"],
                "failed": {
                    "step": "metadata_update",
                    "code": "rate_limited",
                    "message": "Rate limited by Suno — wait and retry"
                },
                "recovery": {
                    "resumable": true,
                    "command": "sunox clip set",
                    "arguments": {
                        "clip_id": "clip-1",
                        "title": "Final title",
                        "lyrics": "Final lyrics",
                        "image_url": "https://cdn.example/cover.jpeg"
                    }
                }
            })
        );
    }

    #[test]
    fn processing_wait_failure_exposes_read_only_inspection_without_claiming_resume() {
        let error = upload_stage_error(
            "upload-1",
            None,
            &["upload_created", "file_uploaded", "upload_finished"],
            "processing_wait",
            CliError::GenerationFailed("timed out".into()),
        );

        let recovery = &error.details().expect("upload recovery")["recovery"];
        assert_eq!(recovery["resumable"], false);
        assert_eq!(
            recovery["reason"],
            "clip initialization cannot be safely replayed"
        );
        assert_eq!(
            recovery["inspection"]["command"],
            "sunox clip upload-status"
        );
        assert_eq!(recovery["inspection"]["arguments"]["upload_id"], "upload-1");
    }

    #[tokio::test]
    async fn upload_workflow_rejects_zero_timeout_before_file_io() {
        let client = SunoClient::new_for_tests(
            "http://127.0.0.1:9".into(),
            AuthState {
                jwt: Some("test-jwt".into()),
                ..AuthState::default()
            },
        )
        .expect("test client");

        let error = run(
            &client,
            UploadWorkflowInput {
                file: Path::new("missing.wav"),
                upload_type: "file_upload",
                is_stem_mix: false,
                title: None,
                lyrics: None,
                timeout: Duration::ZERO,
                poll_interval: Duration::from_secs(1),
            },
        )
        .await
        .expect_err("zero timeout must be rejected before reading the file");

        assert!(
            matches!(error, CliError::Config(message) if message.contains("poll timeout") && message.contains("greater than 0"))
        );
    }

    #[tokio::test]
    async fn upload_poll_deadline_bounds_an_in_flight_request() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind upload status server");
        let address = listener.local_addr().expect("upload status address");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept upload poll");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
            let body = r#"{"id":"upload-1","status":"pending"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
        let client = SunoClient::new_for_tests(
            format!("http://{address}"),
            AuthState {
                jwt: Some("test-jwt".into()),
                ..AuthState::default()
            },
        )
        .expect("test client");

        let error = timeout(
            Duration::from_millis(50),
            wait_until_complete(
                &client,
                "upload-1",
                Duration::from_millis(10),
                Duration::from_millis(1),
            ),
        )
        .await
        .expect("upload polling deadline must bound the request")
        .expect_err("delayed upload status must time out");

        assert!(
            matches!(error, CliError::GenerationFailed(message) if message.contains("did not complete"))
        );
    }
}
