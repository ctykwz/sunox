use serde::Serialize;

use crate::api::types::{
    Clip, CoverArtApplyResponse, CoverArtBatchDescriptor, CoverArtBatchSubmission, CoverArtCost,
    CoverArtImageGenerateRequest, CoverArtModelCategory, CoverArtPollResponse, CoverArtPromptImage,
    CoverArtVideoGenerateRequest, SetMetadataRequest, VideoGenerationStatus,
};
use crate::api::{PollingOptions, SunoClient};
use crate::core::{CliError, MutationAmbiguity, run_before_deadline, sleep_before_deadline};

const IMAGE_FEATURE: &str = "generate_song_image";
const VIDEO_FEATURE: &str = "generate_song_video";
const IMAGE_ACTION: &str = "generate_cover_art";
const COVER_ART_BATCH_QUANTITY: usize = 2;
const IMAGE_READBACK: PollingOptions = PollingOptions {
    timeout: std::time::Duration::from_secs(10),
    interval: std::time::Duration::from_secs(1),
};

#[derive(Debug, Serialize)]
pub struct GeneratedClipImage {
    pub image_url: String,
    pub clip: Clip,
}

#[derive(Debug, Serialize)]
pub struct CoverArtGenerationResult {
    pub media_type: String,
    pub model_category: String,
    pub cost: CoverArtCost,
    pub submission: CoverArtBatchSubmission,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<CoverArtPollResponse>,
}

#[derive(Debug, Serialize)]
pub struct AppliedCoverArt {
    pub media_type: String,
    pub media_id: String,
    pub cover_art_session_id: String,
    pub response: CoverArtApplyResponse,
    pub clip: Clip,
}

pub async fn generate_cover_art_image_batch(
    client: &SunoClient,
    clip_id: &str,
    prompt: &str,
    requested_model: Option<&str>,
    prompt_image: Option<CoverArtPromptImage>,
    polling: Option<PollingOptions>,
) -> Result<CoverArtGenerationResult, CliError> {
    validate_visual_source(client, clip_id, IMAGE_FEATURE, Some(IMAGE_ACTION)).await?;
    let configs = client.cover_art_model_configs().await?;
    let model = select_cover_art_model(&configs.image_model_categories, requested_model, "image")?;
    let cost = client.cover_art_image_cost(&model.category).await?;
    ensure_remaining_cover_art_generations(&cost, "image")?;
    let request = CoverArtImageGenerateRequest {
        generated_text_id: None,
        prompt: prompt.to_string(),
        clip_id: clip_id.to_string(),
        quantity: 2,
        image_gen_category: Some(model.category.clone()),
        prompt_images: prompt_image.into_iter().collect(),
        aspect_ratio: Some("1:1".into()),
    };
    let submission = client.submit_cover_art_image(&request).await?;
    let batch = wait_after_cover_art_submit(client, &submission, "image", polling).await?;
    Ok(CoverArtGenerationResult {
        media_type: "image".into(),
        model_category: model.category,
        cost,
        submission,
        batch,
    })
}

pub async fn generate_cover_art_video_batch(
    client: &SunoClient,
    clip_id: &str,
    prompt: &str,
    requested_model: Option<&str>,
    requested_duration: Option<u32>,
    start_image: Option<CoverArtPromptImage>,
    polling: Option<PollingOptions>,
) -> Result<CoverArtGenerationResult, CliError> {
    validate_visual_source(client, clip_id, VIDEO_FEATURE, Some(IMAGE_ACTION)).await?;
    let configs = client.cover_art_model_configs().await?;
    let model = select_cover_art_model(&configs.video_model_categories, requested_model, "video")?;
    if start_image.is_some() && model.image.as_deref() == Some("not_supported") {
        return Err(CliError::Config(format!(
            "cover-art video model `{}` does not support a start image",
            model.category
        )));
    }
    let allowed = if start_image.is_some() {
        &model.allowed_durations_with_image
    } else {
        &model.allowed_durations
    };
    let duration = select_cover_art_duration(allowed, requested_duration)?;
    let cost = client
        .cover_art_video_cost(&model.category, duration)
        .await?;
    ensure_remaining_cover_art_generations(&cost, "video")?;
    let request = CoverArtVideoGenerateRequest {
        generated_text_id: None,
        prompt_start_image: start_image,
        clip_id: Some(clip_id.to_string()),
        prompt: prompt.to_string(),
        quantity: 2,
        video_gen_category: Some(model.category.clone()),
        duration,
        clip_start_time: None,
        clip_end_time: None,
        aspect_ratio: Some("1:1".into()),
    };
    let submission = client.submit_cover_art_video(&request).await?;
    let batch = wait_after_cover_art_submit(client, &submission, "video", polling).await?;
    Ok(CoverArtGenerationResult {
        media_type: "video".into(),
        model_category: model.category,
        cost,
        submission,
        batch,
    })
}

async fn wait_after_cover_art_submit(
    client: &SunoClient,
    submission: &CoverArtBatchSubmission,
    media_type: &'static str,
    polling: Option<PollingOptions>,
) -> Result<Option<CoverArtPollResponse>, CliError> {
    let Some(polling) = polling else {
        return Ok(None);
    };
    let descriptor = CoverArtBatchDescriptor::new(&submission.batch_id, media_type);
    let expected_ids = if media_type == "image" {
        submission.image_ids.as_slice()
    } else {
        submission.video_ids.as_slice()
    };
    wait_for_cover_art_batch_with_expected_ids(client, &descriptor, expected_ids, polling)
        .await
        .map(Some)
        .map_err(|error| CliError::PartialMutation {
            message: format!(
                "cover-art {media_type} batch {} was submitted but observation failed",
                submission.batch_id
            ),
            details: serde_json::json!({
                "operation": format!("cover_art_{media_type}_generate"),
                "batch_id": submission.batch_id,
                "image_ids": submission.image_ids,
                "video_ids": submission.video_ids,
                "failed": {"step":"batch_poll", "code":error.error_code(), "message":error.to_string()},
                "recovery": {
                    "resumable": true,
                    "command": format!("sunox clip cover-art status {} --media {media_type} --wait --json", submission.batch_id)
                }
            }),
        })
}

pub async fn wait_for_cover_art_batch(
    client: &SunoClient,
    descriptor: &CoverArtBatchDescriptor,
    polling: PollingOptions,
) -> Result<CoverArtPollResponse, CliError> {
    wait_for_cover_art_batch_with_expected_ids(client, descriptor, &[], polling).await
}

async fn wait_for_cover_art_batch_with_expected_ids(
    client: &SunoClient,
    descriptor: &CoverArtBatchDescriptor,
    expected_ids: &[String],
    polling: PollingOptions,
) -> Result<CoverArtPollResponse, CliError> {
    let deadline = polling.deadline()?;
    loop {
        let response = run_before_deadline(
            deadline,
            client.poll_cover_art_batches(std::slice::from_ref(descriptor)),
            cover_art_timeout(&descriptor.id),
        )
        .await?;
        let items = response
            .batches
            .get(&descriptor.id)
            .expect("API validates requested batch identity");
        if items.iter().any(|item| item.is_error()) {
            return Err(CliError::GenerationFailed(format!(
                "cover-art batch {} reached error status",
                descriptor.id
            )));
        }
        let all_expected_complete = if expected_ids.is_empty() {
            items.len() >= COVER_ART_BATCH_QUANTITY && items.iter().all(|item| item.is_complete())
        } else {
            expected_ids.iter().all(|expected_id| {
                items
                    .iter()
                    .find(|item| item.id == *expected_id)
                    .is_some_and(|item| item.is_complete())
            })
        };
        if all_expected_complete {
            return Ok(response);
        }
        if !sleep_before_deadline(deadline, polling.interval).await {
            return Err(cover_art_timeout(&descriptor.id));
        }
    }
}

pub async fn apply_cover_art_image(
    client: &SunoClient,
    clip_id: &str,
    batch_id: &str,
    image_id: &str,
) -> Result<AppliedCoverArt, CliError> {
    validate_visual_source(client, clip_id, IMAGE_FEATURE, Some(IMAGE_ACTION)).await?;
    prove_cover_art_result(client, clip_id, batch_id, "image", image_id).await?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let response = client
        .apply_generated_cover_image(clip_id, image_id, &session_id)
        .await?;
    readback_applied_cover(client, clip_id, "image", image_id, &session_id, response).await
}

pub async fn apply_cover_art_video(
    client: &SunoClient,
    clip_id: &str,
    batch_id: &str,
    video_upload_id: &str,
) -> Result<AppliedCoverArt, CliError> {
    validate_visual_source(client, clip_id, VIDEO_FEATURE, Some(IMAGE_ACTION)).await?;
    prove_cover_art_result(client, clip_id, batch_id, "video", video_upload_id).await?;
    let session_id = uuid::Uuid::new_v4().to_string();
    let response = client
        .apply_generated_cover_video(clip_id, video_upload_id, &session_id)
        .await?;
    readback_applied_cover(
        client,
        clip_id,
        "video",
        video_upload_id,
        &session_id,
        response,
    )
    .await
}

async fn prove_cover_art_result(
    client: &SunoClient,
    clip_id: &str,
    batch_id: &str,
    media_type: &str,
    media_id: &str,
) -> Result<(), CliError> {
    let descriptor = CoverArtBatchDescriptor::new(batch_id, media_type);
    let response = client.poll_cover_art_batches(&[descriptor]).await?;
    let proven = response
        .batches
        .get(batch_id)
        .into_iter()
        .flatten()
        .any(|item| {
            let exact_media = if media_type == "image" {
                item.media_type == "image" && item.id == media_id
            } else {
                matches!(item.media_type.as_str(), "video" | "image-to-video")
                    && item.video_upload_id.as_deref() == Some(media_id)
            };
            exact_media && item.clip_id.as_deref() == Some(clip_id) && item.is_complete()
        });
    if !proven {
        return Err(CliError::Config(format!(
            "cover-art batch `{batch_id}` does not prove that completed {media_type} `{media_id}` belongs to clip `{clip_id}`"
        )));
    }
    Ok(())
}

async fn readback_applied_cover(
    client: &SunoClient,
    clip_id: &str,
    media_type: &'static str,
    media_id: &str,
    session_id: &str,
    response: CoverArtApplyResponse,
) -> Result<AppliedCoverArt, CliError> {
    let expected_url = if media_type == "image" {
        response.image_url.as_str()
    } else {
        response
            .video_cover_url
            .as_deref()
            .expect("video apply response is validated")
    };
    let deadline = IMAGE_READBACK.deadline()?;
    let last_error = loop {
        let observed = run_before_deadline(
            deadline,
            client.get_clip(clip_id),
            cover_art_apply_timeout(clip_id),
        )
        .await;
        let error = match observed {
            Ok(Some(clip)) if clip.id != clip_id => CliError::Api {
                code: "schema_drift",
                message: format!(
                    "cover-art readback returned clip {} instead of {clip_id}",
                    clip.id
                ),
            },
            Ok(Some(clip)) => {
                let matches = if media_type == "image" {
                    clip.image_url.as_deref() == Some(expected_url)
                } else {
                    clip.video_url.as_deref() == Some(expected_url)
                        || clip
                            .extra
                            .get("video_cover_url")
                            .and_then(serde_json::Value::as_str)
                            == Some(expected_url)
                };
                if matches {
                    return Ok(AppliedCoverArt {
                        media_type: media_type.into(),
                        media_id: media_id.into(),
                        cover_art_session_id: session_id.into(),
                        response,
                        clip,
                    });
                }
                CliError::Api {
                    code: "readback_mismatch",
                    message: format!("clip {clip_id} does not yet expose applied {media_type} URL"),
                }
            }
            Ok(None) => CliError::Api {
                code: "readback_mismatch",
                message: format!("clip {clip_id} was not visible during cover-art readback"),
            },
            Err(error) => error,
        };
        if !sleep_before_deadline(deadline, IMAGE_READBACK.interval).await {
            break error;
        }
    };
    Err(CliError::PartialMutation {
        message: format!("cover-art {media_type} was accepted but clip readback did not converge"),
        details: serde_json::json!({
            "operation":"cover_art_apply",
            "clip_id":clip_id,
            "media_type":media_type,
            "media_id":media_id,
            "cover_art_session_id":session_id,
            "failed":{"step":"clip_readback","code":last_error.error_code(),"message":last_error.to_string()},
            "recovery":{"resumable":false,"inspection_commands":[format!("sunox clip info {clip_id} --json")]}
        }),
    })
}

fn select_cover_art_model(
    models: &[CoverArtModelCategory],
    requested: Option<&str>,
    media_type: &str,
) -> Result<CoverArtModelCategory, CliError> {
    if models.is_empty() {
        return Err(CliError::Config(format!(
            "Suno returned no {media_type} cover-art model categories"
        )));
    }
    let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(models[0].clone());
    };
    if let Some(model) = models.iter().find(|model| model.category == requested) {
        return Ok(model.clone());
    }
    let mut display_matches = models.iter().filter(|model| {
        model
            .display_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(requested))
    });
    let Some(first) = display_matches.next() else {
        return Err(CliError::Config(format!(
            "unknown {media_type} cover-art model `{requested}`; run `sunox clip cover-art models`"
        )));
    };
    if display_matches.next().is_some() {
        return Err(CliError::Config(format!(
            "ambiguous {media_type} cover-art model display name `{requested}`; use its category"
        )));
    }
    Ok(first.clone())
}

fn select_cover_art_duration(allowed: &[u32], requested: Option<u32>) -> Result<u32, CliError> {
    if allowed.is_empty() {
        return Err(CliError::Config(
            "selected video cover-art model returned no allowed durations".into(),
        ));
    }
    let duration = requested.unwrap_or_else(|| {
        allowed
            .iter()
            .copied()
            .find(|duration| *duration == 5)
            .unwrap_or(allowed[0])
    });
    if !allowed.contains(&duration) {
        return Err(CliError::Config(format!(
            "video duration {duration} is not allowed by the selected cover-art model; allowed: {allowed:?}"
        )));
    }
    Ok(duration)
}

fn ensure_remaining_cover_art_generations(
    cost: &CoverArtCost,
    media_type: &str,
) -> Result<(), CliError> {
    if cost.remaining_gens == Some(0) {
        return Err(CliError::Config(format!(
            "Suno reports no remaining {media_type} cover-art generations"
        )));
    }
    Ok(())
}

fn cover_art_timeout(batch_id: &str) -> CliError {
    CliError::Api {
        code: "poll_timeout",
        message: format!("timed out waiting for cover-art batch {batch_id}"),
    }
}

fn cover_art_apply_timeout(clip_id: &str) -> CliError {
    CliError::Api {
        code: "readback_timeout",
        message: format!("timed out reading back cover art for clip {clip_id}"),
    }
}

pub async fn generate_and_apply_clip_image(
    client: &SunoClient,
    clip_id: &str,
    prompt: &str,
) -> Result<GeneratedClipImage, CliError> {
    generate_and_apply_clip_image_with_readback(client, clip_id, prompt, IMAGE_READBACK).await
}

pub async fn generate_and_apply_clip_image_with_readback(
    client: &SunoClient,
    clip_id: &str,
    prompt: &str,
    polling: PollingOptions,
) -> Result<GeneratedClipImage, CliError> {
    polling.validate()?;
    validate_visual_source(client, clip_id, IMAGE_FEATURE, Some(IMAGE_ACTION)).await?;

    let generated = client.generate_prompt_image(prompt).await?;
    let image_url = generated.image_url;
    let request = SetMetadataRequest {
        image_url: Some(image_url.clone()),
        ..SetMetadataRequest::default()
    };
    let apply_operation_id = uuid::Uuid::new_v4().to_string();
    if let Err(error) = client.set_metadata_once(clip_id, &request).await {
        let error = if matches!(error, CliError::Http(_) | CliError::Json(_)) {
            MutationAmbiguity::new(
                format!(
                    "clip image apply {apply_operation_id} lost a reliable response; clip {clip_id} may already reference the generated image"
                ),
                "clip_image_apply",
                &apply_operation_id,
                "metadata_submit",
                error.error_code(),
                error.to_string(),
                false,
                "set_metadata has no captured idempotency key; inspect the clip before any replay",
                vec![format!("sunox clip info {clip_id} --json")],
            )
            .with_context("clip_id", serde_json::Value::String(clip_id.to_string()))
            .with_context("image_url", serde_json::Value::String(image_url.clone()))
            .into_error()
        } else {
            error
        };
        return Err(image_partial_error(
            clip_id,
            &image_url,
            &["source_preflight", "image_generated"],
            "metadata_submit",
            error,
            false,
        ));
    }

    let clip = wait_for_clip_image_readback(client, clip_id, &image_url, polling).await?;

    Ok(GeneratedClipImage { image_url, clip })
}

async fn wait_for_clip_image_readback(
    client: &SunoClient,
    clip_id: &str,
    image_url: &str,
    polling: PollingOptions,
) -> Result<Clip, CliError> {
    let deadline = polling.deadline()?;
    let error = loop {
        let observed = run_before_deadline(
            deadline,
            client.get_clip(clip_id),
            clip_image_readback_timeout(clip_id),
        )
        .await;
        let error = match observed {
            Ok(Some(clip))
                if clip.id == clip_id && clip.image_url.as_deref() == Some(image_url) =>
            {
                return Ok(clip);
            }
            Ok(Some(clip)) if clip.id != clip_id => CliError::Api {
                code: "schema_drift",
                message: format!(
                    "generated image readback for `{clip_id}` returned clip `{}`",
                    clip.id
                ),
            },
            Ok(Some(_)) => CliError::Api {
                code: "readback_mismatch",
                message: format!(
                    "clip {clip_id} readback did not yet contain generated image URL {image_url}"
                ),
            },
            Ok(None) => CliError::Api {
                code: "readback_mismatch",
                message: "clip was not visible during generated image readback".into(),
            },
            Err(error) => error,
        };
        if !sleep_before_deadline(deadline, polling.interval).await {
            break error;
        }
    };

    Err(image_partial_error(
        clip_id,
        image_url,
        &["source_preflight", "image_generated", "metadata_accepted"],
        "clip_readback",
        error,
        true,
    ))
}

pub async fn generate_video_and_wait(
    client: &SunoClient,
    clip_id: &str,
    polling: PollingOptions,
) -> Result<VideoGenerationStatus, CliError> {
    polling.validate()?;
    let source = validate_visual_source(client, clip_id, VIDEO_FEATURE, None).await?;
    if !legacy_video_submit_eligibility_is_proven(&source) {
        return Err(CliError::Config(format!(
            "the legacy video POST/status contract is confirmed for clip {clip_id}, but no current response seam proves both clip ownership and download eligibility; refusing to submit generation. Use `sunox clip video-status {clip_id}` for read-only inspection"
        )));
    }
    let operation_id = client.start_video_generation(clip_id).await?;
    wait_for_video(client, clip_id, polling, Some(&operation_id)).await
}

pub async fn wait_for_existing_video(
    client: &SunoClient,
    clip_id: &str,
    polling: PollingOptions,
) -> Result<VideoGenerationStatus, CliError> {
    polling.validate()?;
    wait_for_video(client, clip_id, polling, None).await
}

fn legacy_video_submit_eligibility_is_proven(_source: &Clip) -> bool {
    // The old video manager proves the POST and status routes, but the current
    // bundle does not expose a clip action or ownership predicate that the CLI
    // can reproduce exactly. Do not guess an action name or interpret opaque
    // ownership fields as authorization.
    false
}

async fn validate_visual_source(
    client: &SunoClient,
    clip_id: &str,
    feature: &str,
    action: Option<&str>,
) -> Result<Clip, CliError> {
    let billing = client.billing_info().await?;
    if !billing
        .accessible_features
        .as_ref()
        .is_some_and(|features| features.contains(feature))
    {
        return Err(CliError::Config(format!(
            "Suno does not expose `{feature}` in the current account's accessible_features"
        )));
    }
    let source = client
        .get_clip(clip_id)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("clip {clip_id}")))?;
    if source.id != clip_id {
        return Err(CliError::Api {
            code: "schema_drift",
            message: format!(
                "visual source lookup for `{clip_id}` returned clip `{}`",
                source.id
            ),
        });
    }
    let authenticated_user_id = client.authenticated_user_id().ok_or_else(|| {
        CliError::Config(
            "the authenticated account identity is not present in the current JWT; refusing visual generation"
                .into(),
        )
    })?;
    let source_user_id = source
        .extra
        .get("user_id")
        .and_then(serde_json::Value::as_str);
    if source_user_id != Some(authenticated_user_id.as_str()) {
        return Err(CliError::Config(format!(
            "clip {clip_id} must be explicitly owned by the authenticated account before visual generation"
        )));
    }
    if source.status != "complete" {
        return Err(CliError::Config(format!(
            "clip {clip_id} must be complete before visual generation"
        )));
    }
    if source.is_trashed != Some(false) {
        return Err(CliError::Config(format!(
            "clip {clip_id} must be explicitly non-trashed before visual generation"
        )));
    }
    if let Some(action) = action {
        let enabled = source
            .action_config
            .as_ref()
            .and_then(|config| config.action(action))
            .is_some_and(|action| action.visible == Some(true) && action.disabled == Some(false));
        if !enabled {
            return Err(CliError::Config(format!(
                "clip {clip_id} does not expose an enabled `{action}` action"
            )));
        }
    }
    Ok(source)
}

async fn wait_for_video(
    client: &SunoClient,
    clip_id: &str,
    polling: PollingOptions,
    submitted_operation_id: Option<&str>,
) -> Result<VideoGenerationStatus, CliError> {
    let deadline = polling.deadline()?;
    loop {
        let observed = run_before_deadline(
            deadline,
            client.video_generation_status(clip_id),
            video_timeout(clip_id),
        )
        .await;
        let status = match (observed, submitted_operation_id) {
            (Ok(status), _) => status,
            (Err(error), Some(operation_id)) => {
                return Err(ambiguous_video_observation(operation_id, clip_id, error));
            }
            (Err(error), None) => return Err(error),
        };
        if status.is_failure() {
            return Err(CliError::GenerationFailed(format!(
                "video generation for clip {clip_id} failed with status {}",
                status.status
            )));
        }
        if status.is_complete() {
            return Ok(status);
        }
        if !sleep_before_deadline(deadline, polling.interval).await {
            let error = video_timeout(clip_id);
            return match submitted_operation_id {
                Some(operation_id) => {
                    Err(ambiguous_video_observation(operation_id, clip_id, error))
                }
                None => Err(error),
            };
        }
    }
}

fn image_partial_error(
    clip_id: &str,
    image_url: &str,
    completed_steps: &[&str],
    failed_step: &str,
    error: CliError,
    resumable: bool,
) -> CliError {
    CliError::PartialMutation {
        message: format!(
            "generated clip image for {clip_id} stopped at {failed_step} after {} completed step(s)",
            completed_steps.len()
        ),
        details: serde_json::json!({
            "operation": "clip_generate_image",
            "clip_id": clip_id,
            "image_url": image_url,
            "completed_steps": completed_steps,
            "failed": {
                "step": failed_step,
                "code": error.error_code(),
                "message": error.to_string(),
                "details": error.details(),
            },
            "recovery": {
                "resumable": resumable,
                "reason": if resumable {
                    "only the read-only clip verification remains; resume inspection without replaying metadata"
                } else {
                    "the write state is not proven safe to replay; inspect the clip image first"
                },
                "inspection_commands": [format!("sunox clip info {clip_id} --json")],
                "intended_image_url": image_url,
            }
        }),
    }
}

fn ambiguous_video_observation(operation_id: &str, clip_id: &str, error: CliError) -> CliError {
    MutationAmbiguity::new(
        format!(
            "submitted video generation {operation_id} did not reach a reliable terminal observation for clip {clip_id}"
        ),
        "video_generation",
        operation_id,
        "status_poll",
        error.error_code(),
        error.to_string(),
        true,
        "the clip ID is the captured status handle; resume only the read-only video-status poll",
        vec![
            format!("sunox clip video-status {clip_id} --json"),
            format!("sunox clip video-status {clip_id} --wait --json"),
        ],
    )
    .with_context("clip_id", serde_json::Value::String(clip_id.to_string()))
    .into_error()
}

fn video_timeout(clip_id: &str) -> CliError {
    CliError::GenerationFailed(format!(
        "timed out waiting for video generation for clip {clip_id}"
    ))
}

fn clip_image_readback_timeout(clip_id: &str) -> CliError {
    CliError::Api {
        code: "readback_timeout",
        message: format!("timed out reading generated image state for clip {clip_id}"),
    }
}
