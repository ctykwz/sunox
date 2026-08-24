use serde_json::Value;

use super::SunoClient;
use super::types::{
    CoverArtApplyResponse, CoverArtBatchDescriptor, CoverArtBatchSubmission, CoverArtCost,
    CoverArtHistoryRequest, CoverArtHistoryResponse, CoverArtImageGenerateRequest,
    CoverArtModelConfigs, CoverArtPendingBatches, CoverArtPollResponse,
    CoverArtVideoGenerateRequest, PromptImageRequest, PromptImageResponse, VideoGenerationStatus,
};
use crate::core::{CliError, MutationAmbiguity};

impl SunoClient {
    pub async fn cover_art_model_configs(&self) -> Result<CoverArtModelConfigs, CliError> {
        self.with_auth_retry(|| async {
            let raw: Value = self
                .read_json_with_transport_retry(self.get("/api/video_gen/model-configs"))
                .await?;
            let configs: CoverArtModelConfigs =
                serde_json::from_value(raw).map_err(|error| CliError::Api {
                    code: "schema_drift",
                    message: format!("invalid cover-art model configs: {error}"),
                })?;
            for model in configs
                .image_model_categories
                .iter()
                .chain(configs.video_model_categories.iter())
            {
                if model.category.trim().is_empty() {
                    return Err(CliError::Api {
                        code: "schema_drift",
                        message: "cover-art model config contained a blank category".into(),
                    });
                }
            }
            Ok(configs)
        })
        .await
    }

    pub async fn cover_art_image_cost(&self, category: &str) -> Result<CoverArtCost, CliError> {
        cover_art_category(category)?;
        self.cover_art_cost(
            "/api/video_gen/cost/image",
            serde_json::json!({"image_gen_category": category, "prompt": ""}),
        )
        .await
    }

    pub async fn cover_art_video_cost(
        &self,
        category: &str,
        duration: u32,
    ) -> Result<CoverArtCost, CliError> {
        cover_art_category(category)?;
        if duration == 0 {
            return Err(CliError::Config(
                "cover-art video duration must be greater than zero".into(),
            ));
        }
        self.cover_art_cost(
            "/api/video_gen/cost/video",
            serde_json::json!({"video_gen_category": category, "duration": duration}),
        )
        .await
    }

    async fn cover_art_cost(&self, path: &str, body: Value) -> Result<CoverArtCost, CliError> {
        let response = self.post(path).json(&body).send().await?;
        let response = self.check_cover_art_response(response).await?;
        let cost: CoverArtCost = response.json().await.map_err(|error| CliError::Api {
            code: "schema_drift",
            message: format!("invalid cover-art cost response: {error}"),
        })?;
        if !cost.cost.is_finite() || cost.cost < 0.0 {
            return Err(CliError::Api {
                code: "schema_drift",
                message: "cover-art cost must be a finite non-negative number".into(),
            });
        }
        Ok(cost)
    }

    pub async fn submit_cover_art_image(
        &self,
        request: &CoverArtImageGenerateRequest,
    ) -> Result<CoverArtBatchSubmission, CliError> {
        validate_cover_art_image_request(request)?;
        self.submit_cover_art_batch(
            "/api/video_gen/image/generate",
            request,
            "cover_art_image_generate",
            "image",
        )
        .await
    }

    pub async fn submit_cover_art_video(
        &self,
        request: &CoverArtVideoGenerateRequest,
    ) -> Result<CoverArtBatchSubmission, CliError> {
        validate_cover_art_video_request(request)?;
        self.submit_cover_art_batch(
            "/api/video_gen/video/generate",
            request,
            "cover_art_video_generate",
            "video",
        )
        .await
    }

    async fn submit_cover_art_batch<T: serde::Serialize + ?Sized>(
        &self,
        path: &str,
        request: &T,
        operation: &'static str,
        media_type: &'static str,
    ) -> Result<CoverArtBatchSubmission, CliError> {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let response = self
            .post(path)
            .json(request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_cover_art_submit(
                    &operation_id,
                    operation,
                    media_type,
                    "request_send",
                    error.to_string(),
                )
            })?;
        if response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_cover_art_submit(
                &operation_id,
                operation,
                media_type,
                "response_status",
                format!("HTTP {status}: {body}"),
            ));
        }
        let response = self.check_cover_art_response(response).await?;
        let raw: Value = response.json().await.map_err(|error| {
            ambiguous_cover_art_submit(
                &operation_id,
                operation,
                media_type,
                "response_body",
                error.to_string(),
            )
        })?;
        let submission: CoverArtBatchSubmission = serde_json::from_value(raw).map_err(|error| {
            ambiguous_cover_art_submit(
                &operation_id,
                operation,
                media_type,
                "response_schema",
                error.to_string(),
            )
        })?;
        let expected_ids = if media_type == "image" {
            &submission.image_ids
        } else {
            &submission.video_ids
        };
        if submission.batch_id.trim().is_empty()
            || expected_ids.is_empty()
            || expected_ids.iter().any(|id| id.trim().is_empty())
        {
            return Err(ambiguous_cover_art_submit(
                &operation_id,
                operation,
                media_type,
                "response_schema",
                "the accepted response did not contain a usable batch ID and media IDs".into(),
            ));
        }
        Ok(submission)
    }

    pub async fn pending_cover_art_batches(&self) -> Result<CoverArtPendingBatches, CliError> {
        let pending: CoverArtPendingBatches = self
            .cover_art_read_post("/api/video_gen/pending_batches", &serde_json::json!({}))
            .await?;
        for descriptor in &pending.batch_ids {
            descriptor.validate().map_err(|error| CliError::Api {
                code: "schema_drift",
                message: format!("invalid pending cover-art batch descriptor: {error}"),
            })?;
        }
        Ok(pending)
    }

    pub async fn cover_art_history(
        &self,
        request: &CoverArtHistoryRequest,
    ) -> Result<CoverArtHistoryResponse, CliError> {
        if request.limit == 0 {
            return Err(CliError::Config(
                "cover-art history limit must be greater than zero".into(),
            ));
        }
        if request
            .media_type
            .as_deref()
            .is_some_and(|value| !matches!(value, "image" | "video"))
        {
            return Err(CliError::Config(
                "cover-art history media type must be image or video".into(),
            ));
        }
        self.cover_art_read_post("/api/video_gen/history", request)
            .await
    }

    pub async fn poll_cover_art_batches(
        &self,
        descriptors: &[CoverArtBatchDescriptor],
    ) -> Result<CoverArtPollResponse, CliError> {
        if descriptors.is_empty() {
            return Err(CliError::Config(
                "at least one cover-art batch descriptor is required".into(),
            ));
        }
        for descriptor in descriptors {
            descriptor.validate()?;
        }
        let response: CoverArtPollResponse = self
            .cover_art_read_post(
                "/api/video_gen/poll_batches",
                &serde_json::json!({"batch_ids": descriptors}),
            )
            .await?;
        for descriptor in descriptors {
            let Some(items) = response.batches.get(&descriptor.id) else {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "cover-art poll response omitted requested batch {}",
                        descriptor.id
                    ),
                });
            };
            for item in items {
                if item.id.trim().is_empty()
                    || item.status.trim().is_empty()
                    || !matches!(item.status.as_str(), "processing" | "complete" | "error")
                {
                    return Err(CliError::Api {
                        code: "schema_drift",
                        message: format!(
                            "cover-art batch {} contained an invalid item identity or status",
                            descriptor.id
                        ),
                    });
                }
                if item.is_complete() && item.url.as_deref().is_none_or(|url| url.trim().is_empty())
                {
                    return Err(CliError::Api {
                        code: "schema_drift",
                        message: format!(
                            "completed cover-art item {} had no usable media URL",
                            item.id
                        ),
                    });
                }
            }
        }
        Ok(response)
    }

    async fn cover_art_read_post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl serde::Serialize,
    ) -> Result<T, CliError> {
        let response = self.post(path).json(body).send().await?;
        let response = self.check_response(response).await?;
        response.json().await.map_err(|error| CliError::Api {
            code: "schema_drift",
            message: format!("invalid cover-art read response from {path}: {error}"),
        })
    }

    async fn check_cover_art_response(
        &self,
        response: reqwest::Response,
    ) -> Result<reqwest::Response, CliError> {
        let status = response.status();
        if status.as_u16() != 402 && status.as_u16() != 400 {
            return self.check_response(response).await;
        }
        let text = response.text().await.unwrap_or_default();
        let parsed = serde_json::from_str::<Value>(&text)
            .unwrap_or_else(|_| serde_json::json!({"body": text}));
        let error_type = parsed
            .get("error_type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let code = if status.as_u16() == 402 {
            if error_type == "creation_limit_reached" {
                "creation_limit_reached"
            } else {
                "insufficient_credits"
            }
        } else if error_type == "moderation_error" {
            "moderation_error"
        } else {
            "api_error"
        };
        let message = parsed
            .get("detail")
            .and_then(Value::as_str)
            .or_else(|| {
                parsed
                    .get("moderation_error_message")
                    .and_then(Value::as_str)
            })
            .filter(|message| !message.trim().is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("HTTP {status}: cover-art request rejected"));
        let retryable = parsed.get("retryable").and_then(Value::as_bool);
        Err(CliError::SunoApi {
            code,
            status: status.as_u16(),
            message,
            retryable,
            details: Some(parsed),
        })
    }

    pub async fn apply_generated_cover_image(
        &self,
        clip_id: &str,
        image_id: &str,
        session_id: &str,
    ) -> Result<CoverArtApplyResponse, CliError> {
        self.apply_generated_cover(
            clip_id,
            serde_json::json!({
                "cover_image": {"id": image_id, "type": "generated"},
                "cover_art_session_id": session_id,
            }),
            "image",
            image_id,
        )
        .await
    }

    pub async fn apply_generated_cover_video(
        &self,
        clip_id: &str,
        video_upload_id: &str,
        session_id: &str,
    ) -> Result<CoverArtApplyResponse, CliError> {
        self.apply_generated_cover(
            clip_id,
            serde_json::json!({
                "video_cover_upload_id": video_upload_id,
                "cover_art_session_id": session_id,
            }),
            "video",
            video_upload_id,
        )
        .await
    }

    async fn apply_generated_cover(
        &self,
        clip_id: &str,
        body: Value,
        media_type: &'static str,
        media_id: &str,
    ) -> Result<CoverArtApplyResponse, CliError> {
        for (label, value) in [
            ("clip ID", clip_id),
            ("generated media ID", media_id),
            (
                "cover-art session ID",
                body["cover_art_session_id"].as_str().unwrap_or_default(),
            ),
        ] {
            if value.trim().is_empty() {
                return Err(CliError::Config(format!("{label} must not be empty")));
            }
        }
        let operation_id = uuid::Uuid::new_v4().to_string();
        let response = self
            .post(&format!("/api/gen/{clip_id}/set_metadata/"))
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                ambiguous_cover_art_apply(
                    &operation_id,
                    clip_id,
                    media_type,
                    media_id,
                    "request_send",
                    error.to_string(),
                )
            })?;
        if response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_cover_art_apply(
                &operation_id,
                clip_id,
                media_type,
                media_id,
                "response_status",
                format!("HTTP {status}: {body}"),
            ));
        }
        let response = self.check_cover_art_response(response).await?;
        let applied: CoverArtApplyResponse = response.json().await.map_err(|error| {
            ambiguous_cover_art_apply(
                &operation_id,
                clip_id,
                media_type,
                media_id,
                "response_body",
                error.to_string(),
            )
        })?;
        if applied.image_url.trim().is_empty()
            || (media_type == "video"
                && applied
                    .video_cover_url
                    .as_deref()
                    .is_none_or(|url| url.trim().is_empty()))
        {
            return Err(ambiguous_cover_art_apply(
                &operation_id,
                clip_id,
                media_type,
                media_id,
                "response_schema",
                "metadata response did not contain the applied cover URLs".into(),
            ));
        }
        Ok(applied)
    }

    pub async fn generate_prompt_image(
        &self,
        prompt: &str,
    ) -> Result<PromptImageResponse, CliError> {
        validate_prompt_image_request(prompt)?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = PromptImageRequest { prompt };
        let response = self
            .post("/api/gen/prompt_image/")
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_prompt_image(
                    &operation_id,
                    prompt,
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        let response = self.check_response(response).await?;
        let raw: Value = response.json().await.map_err(|error| {
            ambiguous_prompt_image(
                &operation_id,
                prompt,
                "response_body",
                "http_error",
                error.to_string(),
            )
        })?;
        serde_json::from_value(raw).map_err(|error| {
            ambiguous_prompt_image(
                &operation_id,
                prompt,
                "response_schema",
                "schema_drift",
                error.to_string(),
            )
        })
    }

    /// Start the legacy/current per-clip video generation endpoint exactly once.
    /// The Web caller sends no request body and ignores the response body.
    pub async fn start_video_generation(&self, clip_id: &str) -> Result<String, CliError> {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let path = format!("/api/video/generate/{clip_id}/");
        let response = self.post(&path).send().await.map_err(|error| {
            ambiguous_video_submit(&operation_id, clip_id, "request_send", error.to_string())
        })?;
        self.check_response(response).await?;
        Ok(operation_id)
    }

    pub async fn video_generation_status(
        &self,
        clip_id: &str,
    ) -> Result<VideoGenerationStatus, CliError> {
        let path = format!("/api/video/generate/{clip_id}/status/");
        self.with_auth_retry(|| async {
            let raw: Value = self.read_json_with_transport_retry(self.get(&path)).await?;
            let status: VideoGenerationStatus =
                serde_json::from_value(raw).map_err(|error| CliError::Api {
                    code: "schema_drift",
                    message: format!("invalid video-generation status response: {error}"),
                })?;
            status.validate()
        })
        .await
    }
}

fn cover_art_category(category: &str) -> Result<(), CliError> {
    if category.trim().is_empty() {
        return Err(CliError::Config(
            "cover-art model category must not be empty".into(),
        ));
    }
    Ok(())
}

fn validate_cover_art_image_request(
    request: &CoverArtImageGenerateRequest,
) -> Result<(), CliError> {
    validate_cover_art_common(
        &request.clip_id,
        &request.prompt,
        request.quantity,
        request.image_gen_category.as_deref(),
        !request.prompt_images.is_empty(),
    )?;
    if request.prompt_images.len() > 1 {
        return Err(CliError::Config(
            "cover-art image generation supports one prompt image until the live multi-image flag is confirmed enabled"
                .into(),
        ));
    }
    for image in &request.prompt_images {
        image.validate()?;
    }
    Ok(())
}

fn validate_cover_art_video_request(
    request: &CoverArtVideoGenerateRequest,
) -> Result<(), CliError> {
    validate_cover_art_common(
        request.clip_id.as_deref().unwrap_or_default(),
        &request.prompt,
        request.quantity,
        request.video_gen_category.as_deref(),
        request.prompt_start_image.is_some(),
    )?;
    if request.duration == 0 {
        return Err(CliError::Config(
            "cover-art video duration must be greater than zero".into(),
        ));
    }
    if let Some(image) = &request.prompt_start_image {
        image.validate()?;
    }
    Ok(())
}

fn validate_cover_art_common(
    clip_id: &str,
    prompt: &str,
    quantity: u8,
    category: Option<&str>,
    has_image: bool,
) -> Result<(), CliError> {
    if clip_id.trim().is_empty() {
        return Err(CliError::Config(
            "cover-art clip ID must not be empty".into(),
        ));
    }
    if prompt.encode_utf16().count() > 800 {
        return Err(CliError::Config(
            "cover-art prompt must not exceed 800 UTF-16 code units".into(),
        ));
    }
    if prompt.trim().is_empty() && !has_image {
        return Err(CliError::Config(
            "cover-art generation requires a prompt or prompt image".into(),
        ));
    }
    if quantity != 2 {
        return Err(CliError::Config(
            "cover-art quantity must remain the current song-modal value of 2".into(),
        ));
    }
    cover_art_category(category.unwrap_or_default())
}

fn ambiguous_cover_art_submit(
    operation_id: &str,
    operation: &'static str,
    media_type: &'static str,
    stage: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "cover-art {media_type} operation {operation_id} lost a reliable submit response during {stage}"
        ),
        operation,
        operation_id,
        stage,
        "http_error",
        cause_message,
        false,
        "the batch submit has no client idempotency key; inspect pending batches and history before any replay",
        vec![
            "sunox clip cover-art pending --json".into(),
            format!("sunox clip cover-art history --media {media_type} --json"),
        ],
    )
    .with_context("media_type", Value::String(media_type.into()))
    .into_error()
}

fn ambiguous_cover_art_apply(
    operation_id: &str,
    clip_id: &str,
    media_type: &'static str,
    media_id: &str,
    stage: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "cover-art {media_type} apply {operation_id} lost a reliable response during {stage}"
        ),
        "cover_art_apply",
        operation_id,
        stage,
        "http_error",
        cause_message,
        false,
        "set_metadata has no captured idempotency key; inspect the clip before replaying apply",
        vec![format!("sunox clip info {clip_id} --json")],
    )
    .with_context("clip_id", Value::String(clip_id.into()))
    .with_context("media_type", Value::String(media_type.into()))
    .with_context("media_id", Value::String(media_id.into()))
    .into_error()
}

fn validate_prompt_image_request(prompt: &str) -> Result<(), CliError> {
    let length = prompt.trim().chars().count();
    if length == 0 {
        return Err(CliError::Config("image prompt must not be empty".into()));
    }
    if length > 200 {
        return Err(CliError::Config(
            "image prompt must not exceed the current Web limit of 200 Unicode characters".into(),
        ));
    }
    Ok(())
}

fn ambiguous_prompt_image(
    operation_id: &str,
    prompt: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "prompt image operation {operation_id} lost a reliable response during {stage}; Suno may already have generated an image"
        ),
        "prompt_image_generate",
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "the image endpoint has no captured idempotency key or status handle, so replay may duplicate generation or credit usage",
        vec!["sunox credits --json".into()],
    )
    .with_context("prompt", Value::String(prompt.to_string()))
    .into_error()
}

fn ambiguous_video_submit(
    operation_id: &str,
    clip_id: &str,
    stage: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "video generation operation {operation_id} lost its submit response; generation for clip {clip_id} may already have started"
        ),
        "video_generation",
        operation_id,
        stage,
        "http_error",
        cause_message,
        false,
        "the submit has no captured idempotency key; inspect the source clip status handle before any replay",
        vec![
            format!("sunox clip video-status {clip_id} --json"),
            format!("sunox clip video-status {clip_id} --wait --json"),
        ],
    )
    .with_context("clip_id", Value::String(clip_id.to_string()))
    .into_error()
}
