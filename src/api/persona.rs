use serde_json::Value;

use super::SunoClient;
use super::types::{
    CreatePersonaRequest, EditPersonaRequest, PersonaClipsResponse, PersonaInfo,
    PersonaListResponse, PersonaListScope, ProcessedClipInfo, TogglePersonaLoveResponse,
    TrashPersonasResponse,
};
use crate::core::CliError;

impl SunoClient {
    /// List voice personas.
    /// GET /api/persona/get-personas/?page={page}&continuation_token={token}
    pub async fn list_personas(
        &self,
        scope: PersonaListScope,
        page: u32,
        continuation_token: Option<&str>,
    ) -> Result<PersonaListResponse, CliError> {
        let path = match scope {
            PersonaListScope::Mine => "/api/persona/get-personas/",
            PersonaListScope::Loved => "/api/persona/get-loved-personas/",
            PersonaListScope::Followed => "/api/persona/get-followed-personas/",
        };

        self.with_auth_retry(|| async {
            let mut query = vec![("page", page.to_string())];
            if let Some(token) = continuation_token {
                query.push(("continuation_token", token.to_string()));
            }
            let resp = self.get(path).query(&query).send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Fetch voice persona details.
    /// GET /api/persona/get-persona/{persona_id}/
    pub async fn get_persona(&self, persona_id: &str) -> Result<PersonaInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/persona/get-persona/{persona_id}/"))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            decode_persona(resp.json().await?)
        })
        .await
    }

    /// Fetch voice persona details plus paginated attached clips.
    /// GET /api/persona/get-persona-paginated/{persona_id}/?page={page}
    pub async fn get_persona_clips(
        &self,
        persona_id: &str,
        page: u32,
    ) -> Result<PersonaClipsResponse, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/persona/get-persona-paginated/{persona_id}/"))
                .query(&[("page", page.to_string())])
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Fetch processed vocal clip status and vocal preview URL.
    /// GET /api/processed_clip/{processed_clip_id}
    pub async fn get_processed_clip(
        &self,
        processed_clip_id: &str,
    ) -> Result<ProcessedClipInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/processed_clip/{processed_clip_id}"))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Create a voice persona from an existing clip or voice recording payload.
    /// POST /api/persona/create/
    pub async fn create_persona(
        &self,
        req: &CreatePersonaRequest,
    ) -> Result<PersonaInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self.post("/api/persona/create/").json(req).send().await?;
            let resp = self.check_response(resp).await?;
            decode_persona(resp.json().await?)
        })
        .await
    }

    /// Update voice persona metadata and vocal source fields.
    /// PUT /api/persona/edit-persona/{persona_id}/
    pub async fn edit_persona(&self, req: &EditPersonaRequest) -> Result<PersonaInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .put(&format!("/api/persona/edit-persona/{}/", req.persona_id))
                .json(req)
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            decode_persona(resp.json().await?)
        })
        .await
    }

    /// Toggle loved/favorite state for a persona.
    /// POST /api/persona/{persona_id}/toggle_love/
    pub async fn toggle_persona_love(
        &self,
        persona_id: &str,
    ) -> Result<TogglePersonaLoveResponse, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/persona/{persona_id}/toggle_love/"))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    pub async fn set_persona_love(
        &self,
        persona_id: &str,
        loved: bool,
    ) -> Result<TogglePersonaLoveResponse, CliError> {
        let persona = self.get_persona(persona_id).await?;
        if persona.is_loved == loved {
            return Ok(TogglePersonaLoveResponse {
                loved,
                extra: Default::default(),
            });
        }
        self.toggle_persona_love(persona_id).await
    }

    /// Set persona public/private visibility.
    /// PUT /api/persona/set_visibility/{persona_id}/?is_public={true|false}
    pub async fn set_persona_visibility(
        &self,
        persona_id: &str,
        is_public: bool,
    ) -> Result<PersonaInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .put(&format!("/api/persona/set_visibility/{persona_id}/"))
                .query(&[("is_public", is_public.to_string())])
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            decode_persona(resp.json().await?)
        })
        .await
    }

    /// Move personas to trash.
    /// PUT /api/persona/trash-persona/{persona_id}/?undo=false&hide=false
    pub async fn trash_personas(
        &self,
        persona_ids: &[String],
    ) -> Result<TrashPersonasResponse, CliError> {
        self.update_persona_trash_state(persona_ids, false, false)
            .await
    }

    /// Restore personas from trash.
    /// PUT /api/persona/trash-persona/{persona_id}/?undo=true&hide=false
    pub async fn restore_personas(
        &self,
        persona_ids: &[String],
    ) -> Result<TrashPersonasResponse, CliError> {
        self.update_persona_trash_state(persona_ids, true, false)
            .await
    }

    /// Permanently hide/delete personas from trash.
    /// PUT /api/persona/trash-persona/{persona_id}/?undo=false&hide=true
    pub async fn purge_personas(
        &self,
        persona_ids: &[String],
    ) -> Result<TrashPersonasResponse, CliError> {
        self.update_persona_trash_state(persona_ids, false, true)
            .await
    }

    async fn update_persona_trash_state(
        &self,
        persona_ids: &[String],
        undo: bool,
        hide: bool,
    ) -> Result<TrashPersonasResponse, CliError> {
        let operation = match (undo, hide) {
            (true, _) => "restore_personas",
            (false, true) => "purge_personas",
            (false, false) => "trash_personas",
        };
        let mut result = TrashPersonasResponse {
            updated_persona_ids: Vec::with_capacity(persona_ids.len()),
            voice_persona_count: 0,
            max_voice_personas: 0,
            extra: Default::default(),
        };

        for (index, persona_id) in persona_ids.iter().enumerate() {
            let response = match self
                .with_auth_retry(|| async {
                    let resp = self
                        .put(&format!("/api/persona/trash-persona/{persona_id}/"))
                        .query(&[("undo", undo), ("hide", hide)])
                        .send()
                        .await?;
                    let resp = self.check_response(resp).await?;
                    let body = resp.text().await?;
                    if body.trim().is_empty() {
                        return Ok(None);
                    }
                    Ok(Some(serde_json::from_str::<TrashPersonasResponse>(&body)?))
                })
                .await
            {
                Ok(response) => response,
                Err(error) if result.updated_persona_ids.is_empty() => return Err(error),
                Err(error) => {
                    return Err(CliError::PartialMutation {
                        message: format!(
                            "{operation} completed for {} persona(s), failed for {persona_id}, and left {} persona(s) not attempted",
                            result.updated_persona_ids.len(),
                            persona_ids.len().saturating_sub(index + 1)
                        ),
                        details: serde_json::json!({
                            "operation": operation,
                            "requested_persona_ids": persona_ids,
                            "succeeded_persona_ids": result.updated_persona_ids,
                            "failed": {
                                "persona_id": persona_id,
                                "code": error.error_code(),
                                "message": error.to_string()
                            },
                            "not_attempted_persona_ids": &persona_ids[index + 1..]
                        }),
                    });
                }
            };

            result.updated_persona_ids.push(persona_id.clone());
            if let Some(response) = response {
                result.voice_persona_count = response.voice_persona_count;
                result.max_voice_personas = response.max_voice_personas;
                result.extra.extend(response.extra);
            }
        }

        Ok(result)
    }
}

fn decode_persona(body: Value) -> Result<PersonaInfo, CliError> {
    let candidates = [
        body.get("persona").cloned(),
        body.get("data").cloned(),
        Some(body.clone()),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Ok(persona) = serde_json::from_value::<PersonaInfo>(candidate) {
            return Ok(persona);
        }
    }

    Err(CliError::Api {
        code: "schema_drift",
        message: format!("persona response did not match known Suno schema: {body}"),
    })
}
