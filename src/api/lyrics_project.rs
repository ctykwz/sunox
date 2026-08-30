use std::collections::BTreeSet;

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::SunoClient;
use super::types::{
    FlushLyricsProjectRequest, FlushLyricsProjectResponse, LyricsProject,
    LyricsProjectTitleRequest, LyricsProjectsPage,
};
use crate::core::{CliError, MutationAmbiguity};

impl SunoClient {
    pub async fn lyrics_projects(&self) -> Result<Vec<LyricsProject>, CliError> {
        let mut projects = Vec::new();
        let mut cursor: Option<String> = None;
        let mut seen_cursors = BTreeSet::new();
        loop {
            let mut query = vec![
                ("limit", "50".to_string()),
                ("sort", "updated_at".to_string()),
            ];
            if let Some(cursor) = cursor.as_ref() {
                query.push(("cursor", cursor.clone()));
            }
            let page: LyricsProjectsPage = self
                .with_auth_retry(|| async {
                    let raw: Value = self
                        .read_json_with_transport_retry(
                            self.get("/api/lyrics-projects").query(&query),
                        )
                        .await?;
                    decode_read(raw, "lyrics projects list")
                })
                .await?;
            projects.extend(page.projects);
            let Some(next_cursor) = page
                .next_cursor
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            else {
                return Ok(projects);
            };
            if !seen_cursors.insert(next_cursor.clone()) {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!("lyrics projects pagination repeated cursor `{next_cursor}`"),
                });
            }
            cursor = Some(next_cursor);
        }
    }

    pub async fn lyrics_project(&self, project_id: &str) -> Result<LyricsProject, CliError> {
        self.lyrics_project_optional(project_id)
            .await?
            .ok_or_else(|| CliError::NotFound(format!("lyrics project {project_id}")))
    }

    pub async fn create_lyrics_project(&self, title: &str) -> Result<LyricsProject, CliError> {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = LyricsProjectTitleRequest {
            title: web_title(title),
        };
        let response = self
            .post_without_redirect("/api/lyrics-projects")
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_project_mutation(
                    "lyrics_project_create",
                    &operation_id,
                    None,
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        let response = reject_ambiguous_project_server_error(
            response,
            "lyrics_project_create",
            &operation_id,
            None,
        )
        .await?;
        let response = self.check_response(response).await?;
        let created: LyricsProject =
            decode_mutation_response(response, "lyrics_project_create", &operation_id, None)
                .await?;
        let readback = self.lyrics_project(&created.id).await.map_err(|error| {
            project_readback_error(
                "lyrics_project_create",
                &created.id,
                "project_readback",
                error,
                true,
            )
        })?;
        if readback.title != request.title {
            return Err(project_readback_error(
                "lyrics_project_create",
                &created.id,
                "title_mismatch",
                readback_mismatch("created project title did not match the submitted title"),
                false,
            ));
        }
        Ok(readback)
    }

    pub async fn rename_lyrics_project(
        &self,
        project_id: &str,
        title: &str,
    ) -> Result<LyricsProject, CliError> {
        self.lyrics_project(project_id).await?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let path = format!("/api/lyrics-projects/{project_id}");
        let request = LyricsProjectTitleRequest {
            title: web_title(title),
        };
        let response = self
            .patch_without_redirect(&path)
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_project_mutation(
                    "lyrics_project_rename",
                    &operation_id,
                    Some(project_id),
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        let response = reject_ambiguous_project_server_error(
            response,
            "lyrics_project_rename",
            &operation_id,
            Some(project_id),
        )
        .await?;
        let response = self.check_response(response).await?;
        let updated: LyricsProject = decode_mutation_response(
            response,
            "lyrics_project_rename",
            &operation_id,
            Some(project_id),
        )
        .await?;
        if updated.id != project_id {
            return Err(ambiguous_project_mutation(
                "lyrics_project_rename",
                &operation_id,
                Some(project_id),
                "response_schema",
                "schema_drift",
                format!("rename response returned project ID `{}`", updated.id),
            ));
        }
        let readback = self.lyrics_project(project_id).await.map_err(|error| {
            project_readback_error(
                "lyrics_project_rename",
                project_id,
                "project_readback",
                error,
                true,
            )
        })?;
        if readback.title != request.title {
            return Err(project_readback_error(
                "lyrics_project_rename",
                project_id,
                "title_mismatch",
                readback_mismatch("renamed project title did not match the submitted title"),
                false,
            ));
        }
        Ok(readback)
    }

    pub async fn flush_lyrics_project(
        &self,
        project_id: &str,
        lyrics: &str,
    ) -> Result<FlushLyricsProjectResponse, CliError> {
        self.lyrics_project(project_id).await?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let path = format!("/api/lyrics-projects/{project_id}/flush");
        let request = FlushLyricsProjectRequest { lyrics };
        let response = self
            .post_without_redirect(&path)
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_project_mutation(
                    "lyrics_project_flush",
                    &operation_id,
                    Some(project_id),
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        let response = reject_ambiguous_project_server_error(
            response,
            "lyrics_project_flush",
            &operation_id,
            Some(project_id),
        )
        .await?;
        let response = self.check_response(response).await?;
        let flushed: FlushLyricsProjectResponse = decode_mutation_response(
            response,
            "lyrics_project_flush",
            &operation_id,
            Some(project_id),
        )
        .await?;
        let readback = self.lyrics_project(project_id).await.map_err(|error| {
            project_readback_error(
                "lyrics_project_flush",
                project_id,
                "project_readback",
                error,
                true,
            )
        })?;
        if readback.lyrics != lyrics {
            return Err(project_readback_error(
                "lyrics_project_flush",
                project_id,
                "lyrics_mismatch",
                readback_mismatch("project lyrics did not match the flushed text"),
                false,
            ));
        }
        Ok(flushed)
    }

    pub async fn delete_lyrics_project(&self, project_id: &str) -> Result<(), CliError> {
        self.lyrics_project(project_id).await?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let path = format!("/api/lyrics-projects/{project_id}");
        let response = self
            .delete_without_redirect(&path)
            .send()
            .await
            .map_err(|error| {
                ambiguous_project_mutation(
                    "lyrics_project_delete",
                    &operation_id,
                    Some(project_id),
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        let response = reject_ambiguous_project_server_error(
            response,
            "lyrics_project_delete",
            &operation_id,
            Some(project_id),
        )
        .await?;
        self.check_response(response).await?;
        match self.lyrics_project_optional(project_id).await {
            Ok(None) => Ok(()),
            Ok(Some(_)) => Err(project_readback_error(
                "lyrics_project_delete",
                project_id,
                "state_mismatch",
                readback_mismatch("lyrics project remained readable after delete"),
                false,
            )),
            Err(error) => Err(project_readback_error(
                "lyrics_project_delete",
                project_id,
                "project_readback",
                error,
                true,
            )),
        }
    }

    async fn lyrics_project_optional(
        &self,
        project_id: &str,
    ) -> Result<Option<LyricsProject>, CliError> {
        let path = format!("/api/lyrics-projects/{project_id}");
        self.with_auth_retry(|| async {
            let response = self.get(&path).send().await?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let response = self.check_response(response).await?;
            let raw: Value = response.json().await?;
            let project: LyricsProject = decode_read(raw, "lyrics project")?;
            if project.id != project_id {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "lyrics project lookup for `{project_id}` returned project `{}`",
                        project.id
                    ),
                });
            }
            Ok(Some(project))
        })
        .await
    }
}

fn web_title(title: &str) -> String {
    title.chars().take(200).collect()
}

fn decode_read<T: DeserializeOwned>(raw: Value, label: &str) -> Result<T, CliError> {
    serde_json::from_value(raw).map_err(|error| CliError::Api {
        code: "schema_drift",
        message: format!("invalid {label} response: {error}"),
    })
}

async fn decode_mutation_response<T: DeserializeOwned>(
    response: reqwest::Response,
    operation: &'static str,
    operation_id: &str,
    project_id: Option<&str>,
) -> Result<T, CliError> {
    let raw: Value = response.json().await.map_err(|error| {
        ambiguous_project_mutation(
            operation,
            operation_id,
            project_id,
            "response_body",
            "http_error",
            error.to_string(),
        )
    })?;
    serde_json::from_value(raw).map_err(|error| {
        ambiguous_project_mutation(
            operation,
            operation_id,
            project_id,
            "response_schema",
            "schema_drift",
            error.to_string(),
        )
    })
}

async fn reject_ambiguous_project_server_error(
    response: reqwest::Response,
    operation: &'static str,
    operation_id: &str,
    project_id: Option<&str>,
) -> Result<reqwest::Response, CliError> {
    if response.status().is_redirection() || response.status().is_server_error() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(ambiguous_project_mutation(
            operation,
            operation_id,
            project_id,
            "response_status",
            "http_error",
            format!("HTTP {status}: {body}"),
        ));
    }
    Ok(response)
}

fn ambiguous_project_mutation(
    operation: &'static str,
    operation_id: &str,
    project_id: Option<&str>,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    let mut inspection_commands = vec!["sunox lyrics projects list --json".into()];
    if let Some(project_id) = project_id {
        inspection_commands.insert(0, format!("sunox lyrics projects info {project_id} --json"));
    }
    let mut ambiguity = MutationAmbiguity::new(
        format!(
            "lyrics project operation {operation_id} lost a reliable response during {stage}; the write may already have succeeded"
        ),
        operation,
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "the project write has no captured idempotency key; inspect state before any replay",
        inspection_commands,
    );
    if let Some(project_id) = project_id {
        ambiguity = ambiguity.with_context("project_id", Value::String(project_id.to_string()));
    }
    ambiguity.into_error()
}

fn project_readback_error(
    operation: &'static str,
    project_id: &str,
    failed_step: &'static str,
    error: CliError,
    resumable: bool,
) -> CliError {
    CliError::PartialMutation {
        message: format!(
            "{operation} for {project_id} was accepted but failed business readback at {failed_step}"
        ),
        details: serde_json::json!({
            "operation": operation,
            "project_id": project_id,
            "completed_steps": ["mutation_accepted"],
            "failed": {
                "step": failed_step,
                "code": error.error_code(),
                "message": error.to_string(),
            },
            "recovery": {
                "resumable": resumable,
                "reason": if resumable {
                    "resume only the read-only project inspection"
                } else {
                    "business readback disagreed with the intended state; do not replay automatically"
                },
                "inspection_commands": [
                    format!("sunox lyrics projects info {project_id} --json"),
                    "sunox lyrics projects list --json".to_string()
                ]
            }
        }),
    }
}

fn readback_mismatch(message: &str) -> CliError {
    CliError::Api {
        code: "readback_mismatch",
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::web_title;

    #[test]
    fn project_titles_match_the_web_unicode_character_limit() {
        let title = format!("{}tail", "歌".repeat(200));
        let truncated = web_title(&title);
        assert_eq!(truncated.chars().count(), 200);
        assert!(!truncated.contains("tail"));
    }
}
