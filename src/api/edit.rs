use super::types::Clip;
use super::{PollingOptions, SunoClient};
use crate::core::{CliError, MutationAmbiguity, run_before_deadline, sleep_before_deadline};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Clone, Copy)]
enum EditOperation {
    Reverse,
    Crop,
    Fade,
}

impl EditOperation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reverse => "reverse",
            Self::Crop => "crop",
            Self::Fade => "fade",
        }
    }
}

#[derive(Serialize)]
struct ReverseRequest<'a> {
    clip_id: &'a str,
    title: &'a str,
}

#[derive(Serialize)]
struct CropRequest<'a> {
    crop_start_s: f64,
    crop_end_s: f64,
    is_crop_remove: bool,
    title: &'a str,
    ui_surface: &'static str,
}

#[derive(Serialize)]
struct FadeRequest<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    fade_in_time: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fade_out_time: Option<f64>,
    title: &'a str,
}

#[derive(Deserialize)]
struct EditActionResponse {
    #[serde(deserialize_with = "deserialize_edit_action_id")]
    action_clip_id: String,
}

fn deserialize_edit_action_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let action_clip_id = String::deserialize(deserializer)?;
    if action_clip_id.trim().is_empty() {
        return Err(serde::de::Error::custom(
            "edit action_clip_id must not be empty",
        ));
    }
    Ok(action_clip_id)
}

#[derive(Deserialize)]
struct EditActionStatus {
    status: Option<String>,
}

struct SubmittedEdit<Response> {
    response: Response,
    operation_id: String,
}

impl SunoClient {
    pub async fn reverse_clip(&self, clip_id: &str, title: &str) -> Result<Clip, CliError> {
        let req = ReverseRequest { clip_id, title };
        let submitted: SubmittedEdit<Clip> = self
            .submit_edit(
                EditOperation::Reverse,
                clip_id,
                "/api/clips/reverse-clip/",
                &req,
            )
            .await?;
        if submitted.response.id.trim().is_empty() {
            return Err(ambiguous_edit_submit_details(
                EditOperation::Reverse,
                &submitted.operation_id,
                clip_id,
                "response_schema",
                "schema_drift",
                "reverse response contained an empty clip ID".into(),
            ));
        }
        Ok(submitted.response)
    }

    pub async fn crop_clip(
        &self,
        clip_id: &str,
        start_s: f64,
        end_s: f64,
        remove_section: bool,
        title: &str,
        polling: PollingOptions,
    ) -> Result<Clip, CliError> {
        polling.validate()?;
        let req = CropRequest {
            crop_start_s: start_s,
            crop_end_s: end_s,
            is_crop_remove: remove_section,
            title,
            ui_surface: "song_actions",
        };
        let path = format!("/api/edit/crop/{clip_id}/");
        let action: EditActionResponse = self
            .submit_edit(EditOperation::Crop, clip_id, &path, &req)
            .await?
            .response;
        self.wait_for_edit_action(EditOperation::Crop, &action.action_clip_id, polling)
            .await
    }

    pub async fn fade_clip(
        &self,
        clip_id: &str,
        fade_in_time: Option<f64>,
        fade_out_time: Option<f64>,
        title: &str,
        polling: PollingOptions,
    ) -> Result<Clip, CliError> {
        polling.validate()?;
        let req = FadeRequest {
            fade_in_time,
            fade_out_time,
            title,
        };
        let path = format!("/api/edit/fade/{clip_id}/");
        let action: EditActionResponse = self
            .submit_edit(EditOperation::Fade, clip_id, &path, &req)
            .await?
            .response;
        self.wait_for_edit_action(EditOperation::Fade, &action.action_clip_id, polling)
            .await
    }

    async fn submit_edit<Request, Response>(
        &self,
        operation: EditOperation,
        source_clip_id: &str,
        path: &str,
        request: &Request,
    ) -> Result<SubmittedEdit<Response>, CliError>
    where
        Request: Serialize + ?Sized,
        Response: DeserializeOwned,
    {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let response = {
            let request = self.post_without_redirect(path).json(request);
            let resp = self
                .prepare_mutation_request(request)
                .await?
                .send()
                .await
                .map_err(|error| {
                    ambiguous_edit_submit(
                        operation,
                        &operation_id,
                        source_clip_id,
                        "request_send",
                        error,
                    )
                })?;
            if resp.status().is_redirection() || resp.status().is_server_error() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(ambiguous_edit_submit_details(
                    operation,
                    &operation_id,
                    source_clip_id,
                    "response_status",
                    "http_error",
                    format!("HTTP {status}: {body}"),
                ));
            }
            let resp = self.check_response(resp).await?;
            resp.json().await.map_err(|error| {
                ambiguous_edit_submit(
                    operation,
                    &operation_id,
                    source_clip_id,
                    "response_body",
                    error,
                )
            })
        }?;
        Ok(SubmittedEdit {
            response,
            operation_id,
        })
    }

    async fn wait_for_edit_action(
        &self,
        operation: EditOperation,
        action_clip_id: &str,
        polling: PollingOptions,
    ) -> Result<Clip, CliError> {
        let path = format!("/api/edit/action/{action_clip_id}/");
        let deadline = polling.deadline()?;
        loop {
            let action_status: EditActionStatus = run_before_deadline(
                deadline,
                self.with_auth_retry(|| async {
                    let resp = self.get(&path).send().await?;
                    let resp = self.check_response(resp).await?;
                    Ok(resp.json().await?)
                }),
                edit_action_timeout(action_clip_id),
            )
            .await
            .map_err(|error| {
                ambiguous_edit_poll(operation, action_clip_id, "action_poll", error)
            })?;
            if edit_status_failed(action_status.status.as_deref()) {
                return Err(CliError::GenerationFailed(format!(
                    "edit action {action_clip_id} failed"
                )));
            }
            if action_status.status.as_deref() == Some("complete") {
                break;
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                return Err(ambiguous_edit_poll(
                    operation,
                    action_clip_id,
                    "action_poll",
                    edit_action_timeout(action_clip_id),
                ));
            }
        }

        loop {
            let result_clip = run_before_deadline(
                deadline,
                self.edit_result_clip(action_clip_id),
                edit_result_timeout(action_clip_id),
            )
            .await
            .map_err(|error| {
                ambiguous_edit_poll(operation, action_clip_id, "result_poll", error)
            })?;
            if let Some(clip) = result_clip {
                if edit_status_failed(Some(&clip.status)) {
                    return Err(CliError::GenerationFailed(format!(
                        "edit result clip {action_clip_id} failed with status {}",
                        clip.status
                    )));
                }
                if clip.status == "complete" {
                    return Ok(clip);
                }
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                return Err(ambiguous_edit_poll(
                    operation,
                    action_clip_id,
                    "result_poll",
                    edit_result_timeout(action_clip_id),
                ));
            }
        }
    }

    async fn edit_result_clip(&self, action_clip_id: &str) -> Result<Option<Clip>, CliError> {
        let requested = [action_clip_id.to_string()];
        Ok(self
            .get_clips(&requested)
            .await?
            .into_iter()
            .find(|clip| clip.id == action_clip_id))
    }
}

fn ambiguous_edit_poll(
    operation: EditOperation,
    action_clip_id: &str,
    stage: &'static str,
    error: CliError,
) -> CliError {
    let cause_code = error.error_code();
    let cause_message = error.to_string();
    MutationAmbiguity::new(
        format!(
            "{} action {action_clip_id} did not reach a reliable terminal result during {stage}; the edit may still complete",
            operation.as_str()
        ),
        operation.as_str(),
        action_clip_id,
        stage,
        cause_code,
        cause_message,
        true,
        "resume by reading the returned action/result clip id; do not submit the edit again",
        vec![
            format!("sunox clip status {action_clip_id} --json"),
            format!("sunox clip wait {action_clip_id} --json"),
        ],
    )
    .with_context(
        "action_clip_id",
        serde_json::Value::String(action_clip_id.to_string()),
    )
    .into_error()
}

fn ambiguous_edit_submit(
    operation: EditOperation,
    operation_id: &str,
    source_clip_id: &str,
    stage: &'static str,
    error: reqwest::Error,
) -> CliError {
    ambiguous_edit_submit_details(
        operation,
        operation_id,
        source_clip_id,
        stage,
        "http_error",
        error.to_string(),
    )
}

fn ambiguous_edit_submit_details(
    operation: EditOperation,
    operation_id: &str,
    source_clip_id: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "{} operation {operation_id} lost a reliable response during {stage}; Suno may still have created an edited clip",
            operation.as_str()
        ),
        operation.as_str(),
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "the edit submit has no client idempotency key, so replay may create a duplicate clip",
        vec!["sunox clip list --json".into()],
    )
    .with_context(
        "source_clip_id",
        serde_json::Value::String(source_clip_id.to_string()),
    )
    .into_error()
}

fn edit_action_timeout(action_clip_id: &str) -> CliError {
    CliError::GenerationFailed(format!(
        "timed out waiting for edit action {action_clip_id}"
    ))
}

fn edit_result_timeout(action_clip_id: &str) -> CliError {
    CliError::GenerationFailed(format!(
        "timed out waiting for edit result clip {action_clip_id}"
    ))
}

fn edit_status_failed(status: Option<&str>) -> bool {
    matches!(status, Some("error" | "failed"))
}
