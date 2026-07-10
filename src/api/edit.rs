use super::types::Clip;
use super::{PollingOptions, SunoClient};
use crate::core::{CliError, run_before_deadline, sleep_before_deadline};
use serde::{Deserialize, Serialize};

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
    action_clip_id: String,
}

#[derive(Deserialize)]
struct EditActionStatus {
    status: Option<String>,
}

impl SunoClient {
    pub async fn reverse_clip(&self, clip_id: &str, title: &str) -> Result<Clip, CliError> {
        let req = ReverseRequest { clip_id, title };
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/clips/reverse-clip/")
                .json(&req)
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
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
        let action = self
            .with_auth_retry(|| async {
                let resp = self.post(&path).json(&req).send().await?;
                let resp = self.check_response(resp).await?;
                Ok(resp.json::<EditActionResponse>().await?)
            })
            .await?;
        self.wait_for_edit_action(&action.action_clip_id, polling)
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
        let action = self
            .with_auth_retry(|| async {
                let resp = self.post(&path).json(&req).send().await?;
                let resp = self.check_response(resp).await?;
                Ok(resp.json::<EditActionResponse>().await?)
            })
            .await?;
        self.wait_for_edit_action(&action.action_clip_id, polling)
            .await
    }

    async fn wait_for_edit_action(
        &self,
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
            .await?;
            if edit_status_failed(action_status.status.as_deref()) {
                return Err(CliError::GenerationFailed(format!(
                    "edit action {action_clip_id} failed"
                )));
            }
            if action_status.status.as_deref() == Some("complete") {
                break;
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                return Err(CliError::GenerationFailed(format!(
                    "timed out waiting for edit action {action_clip_id}"
                )));
            }
        }

        loop {
            let result_clip = run_before_deadline(
                deadline,
                self.edit_result_clip(action_clip_id),
                edit_result_timeout(action_clip_id),
            )
            .await?;
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
                return Err(CliError::GenerationFailed(format!(
                    "timed out waiting for edit result clip {action_clip_id}"
                )));
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
