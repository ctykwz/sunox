use serde_json::Value;

use super::SunoClient;
use super::mutation::MutationSpec;
use super::types::{
    CreatePersonaRequest, EditPersonaRequest, PersonaClipsResponse, PersonaInfo,
    PersonaListResponse, PersonaListScope, TogglePersonaLoveResponse, TrashPersonasResponse,
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
        if continuation_token.is_some() && !matches!(scope, PersonaListScope::Mine) {
            return Err(CliError::Config(
                "Suno Web continuation tokens apply only to the owned Persona collection; loved and followed collections use page numbers only"
                    .into(),
            ));
        }
        let path = match scope {
            PersonaListScope::Mine => "/api/persona/get-personas/",
            PersonaListScope::Loved => "/api/persona/get-loved-personas/",
            PersonaListScope::Followed => "/api/persona/get-followed-personas/",
        };

        self.with_auth_retry(|| async {
            let mut query = vec![("page", page.to_string())];
            if let Some(token) =
                continuation_token.filter(|_| matches!(scope, PersonaListScope::Mine))
            {
                query.push(("continuation_token", token.to_string()));
            }
            self.read_json_with_transport_retry(self.get(path).query(&query))
                .await
        })
        .await
    }

    /// Fetch voice persona details.
    /// GET /api/persona/get-persona/{persona_id}/
    pub async fn get_persona(&self, persona_id: &str) -> Result<PersonaInfo, CliError> {
        self.with_auth_retry(|| async {
            let raw = self
                .read_json_with_transport_retry(
                    self.get(&format!("/api/persona/get-persona/{persona_id}/")),
                )
                .await?;
            decode_persona(raw)
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
            self.read_json_with_transport_retry(
                self.get(&format!("/api/persona/get-persona-paginated/{persona_id}/"))
                    .query(&[("page", page.to_string())]),
            )
            .await
        })
        .await
    }

    /// Create a voice persona from an existing clip or voice recording payload.
    /// POST /api/persona/create/
    pub async fn create_persona(
        &self,
        req: &CreatePersonaRequest,
    ) -> Result<PersonaInfo, CliError> {
        let spec = persona_mutation_spec("persona_create", None);
        let body = self
            .mutation_json_once(
                self.post_without_redirect("/api/persona/create/").json(req),
                &spec,
            )
            .await?;
        let persona = decode_persona(body).map_err(|error| {
            spec.ambiguous("response_schema", error.error_code(), error.to_string())
        })?;
        if persona.id.trim().is_empty() {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                "persona create returned a blank id".into(),
            ));
        }
        Ok(persona)
    }

    /// Update voice persona metadata and vocal source fields.
    /// PUT /api/persona/edit-persona/{persona_id}/
    pub async fn edit_persona(&self, req: &EditPersonaRequest) -> Result<PersonaInfo, CliError> {
        let spec = persona_mutation_spec("persona_edit", Some(&req.persona_id));
        let body = self
            .mutation_json_once(
                self.put_without_redirect(&format!(
                    "/api/persona/edit-persona/{}/",
                    req.persona_id
                ))
                .json(req),
                &spec,
            )
            .await?;
        let persona = decode_persona(body).map_err(|error| {
            spec.ambiguous("response_schema", error.error_code(), error.to_string())
        })?;
        if persona.id != req.persona_id {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                format!("persona edit returned id `{}`", persona.id),
            ));
        }
        Ok(persona)
    }

    /// Toggle loved/favorite state for a persona.
    /// POST /api/persona/{persona_id}/toggle_love/
    pub async fn toggle_persona_love(
        &self,
        persona_id: &str,
    ) -> Result<TogglePersonaLoveResponse, CliError> {
        let spec = persona_mutation_spec("persona_toggle_love", Some(persona_id));
        self.mutation_json_once(
            self.post_without_redirect(&format!("/api/persona/{persona_id}/toggle_love/")),
            &spec,
        )
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
        let response = self.toggle_persona_love(persona_id).await?;
        if response.loved != loved {
            let spec = persona_mutation_spec("persona_set_love", Some(persona_id))
                .with_context("loved", serde_json::json!(loved));
            return Err(spec.ambiguous(
                "response_state",
                "state_mismatch",
                format!("persona love response returned loved={}", response.loved),
            ));
        }
        Ok(response)
    }

    /// Set persona public/private visibility.
    /// PUT /api/persona/set_visibility/{persona_id}/?is_public={true|false}
    pub async fn set_persona_visibility(
        &self,
        persona_id: &str,
        is_public: bool,
    ) -> Result<PersonaInfo, CliError> {
        let spec = persona_mutation_spec("persona_set_visibility", Some(persona_id))
            .with_context("is_public", serde_json::json!(is_public));
        let body = self
            .mutation_json_once(
                self.put_without_redirect(&format!("/api/persona/set_visibility/{persona_id}/"))
                    .query(&[("is_public", is_public.to_string())]),
                &spec,
            )
            .await?;
        let persona = decode_persona(body).map_err(|error| {
            spec.ambiguous("response_schema", error.error_code(), error.to_string())
        })?;
        if persona.id != persona_id || persona.is_public != Some(is_public) {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                format!(
                    "persona visibility response returned id `{}` and is_public {:?}",
                    persona.id, persona.is_public
                ),
            ));
        }
        Ok(persona)
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
            let spec = persona_mutation_spec(operation, Some(persona_id))
                .with_context("undo", serde_json::json!(undo))
                .with_context("hide", serde_json::json!(hide));
            let response = match self
                .mutation_text_once(
                    self.put_without_redirect(&format!("/api/persona/trash-persona/{persona_id}/"))
                        .query(&[("undo", undo), ("hide", hide)]),
                    &spec,
                )
                .await
                .and_then(|body| {
                    if body.trim().is_empty() {
                        return Err(spec.ambiguous(
                            "response_body",
                            "schema_drift",
                            "persona trash mutation returned an empty accepted response".into(),
                        ));
                    }
                    let response =
                        serde_json::from_str::<TrashPersonasResponse>(&body).map_err(|error| {
                            spec.ambiguous("response_body", "schema_drift", error.to_string())
                        })?;
                    if !response.updated_persona_ids.contains(persona_id) {
                        return Err(spec.ambiguous(
                            "response_state",
                            "state_mismatch",
                            format!(
                                "persona trash response did not confirm requested id `{persona_id}`"
                            ),
                        ));
                    }
                    Ok(response)
                }) {
                Ok(response) => response,
                Err(error) if result.updated_persona_ids.is_empty() => return Err(error),
                Err(error) => {
                    let mut failed = serde_json::json!({
                        "persona_id": persona_id,
                        "code": error.error_code(),
                        "message": error.to_string()
                    });
                    if let Some(error_details) = error.details() {
                        failed["details"] = error_details.clone();
                    }
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
                            "failed": failed,
                            "not_attempted_persona_ids": &persona_ids[index + 1..]
                        }),
                    });
                }
            };

            result.updated_persona_ids.push(persona_id.clone());
            result.voice_persona_count = response.voice_persona_count;
            result.max_voice_personas = response.max_voice_personas;
            result.extra.extend(response.extra);
        }

        Ok(result)
    }
}

fn persona_mutation_spec(operation: &'static str, persona_id: Option<&str>) -> MutationSpec {
    let resource = persona_id
        .map(|id| format!("persona {id}"))
        .unwrap_or_else(|| "new persona".to_string());
    let commands = persona_id
        .map(|id| vec![format!("sunox persona info {id} --json")])
        .unwrap_or_else(|| vec!["sunox persona list --json".into()]);
    let mut spec = MutationSpec::new(operation, resource, commands);
    if let Some(persona_id) = persona_id {
        spec = spec.with_context(
            "persona_id",
            serde_json::Value::String(persona_id.to_string()),
        );
    }
    spec
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
