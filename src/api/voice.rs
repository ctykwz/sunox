use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::SunoClient;
use super::types::{
    AudioUploadInitResponse, CreateAudioUploadRequest, CreatePersonaRequest,
    CreateVoiceVerificationRequest, FinishAudioUploadRequest, PersonaInfo,
    ProcessVoiceSampleRequest, ProcessVoiceSampleResponse,
    ProcessVoiceVerificationRecordingRequest, ProcessVoiceVerificationRecordingResponse,
    ProcessedVoiceStatus, VoicePhrase, VoiceVerification,
};
use crate::core::{CliError, MutationAmbiguity};

impl SunoClient {
    /// Fetch the current dynamic phrase that must be spoken for Voice verification.
    pub async fn get_voice_phrase(&self, language: &str) -> Result<VoicePhrase, CliError> {
        self.with_auth_retry(|| async {
            self.read_json_with_transport_retry(
                self.get("/api/voice-verification/phrase/")
                    .query(&[("language", language)]),
            )
            .await
        })
        .await
    }

    /// Inspect processing state for a Voice sample created by voice-vox-stem.
    pub async fn get_processed_voice_status(
        &self,
        processed_clip_id: &str,
    ) -> Result<ProcessedVoiceStatus, CliError> {
        let status: ProcessedVoiceStatus = self
            .with_auth_retry(|| async {
                self.read_json_with_transport_retry(
                    self.get(&format!("/api/processed_clip/{processed_clip_id}")),
                )
                .await
            })
            .await?;
        if let Some(actual_id) = status.id.as_deref()
            && actual_id != processed_clip_id
        {
            return Err(identity_mismatch(
                "processed Voice status",
                processed_clip_id,
                actual_id,
            ));
        }
        Ok(status)
    }

    /// Inspect the current status of a Voice ownership verification.
    pub async fn get_voice_verification(
        &self,
        verification_id: &str,
    ) -> Result<VoiceVerification, CliError> {
        let verification: VoiceVerification = self
            .with_auth_retry(|| async {
                self.read_json_with_transport_retry(
                    self.get(&format!("/api/voice-verification/{verification_id}")),
                )
                .await
            })
            .await?;
        if verification.id != verification_id {
            return Err(identity_mismatch(
                "Voice verification",
                verification_id,
                &verification.id,
            ));
        }
        Ok(verification)
    }

    /// Create the presigned upload used by a Voice workflow.
    ///
    /// This deliberately does not use `with_auth_retry`: the endpoint creates
    /// a server resource and has no verified idempotency key.
    pub(crate) async fn create_voice_recording_upload(
        &self,
        workflow_id: &str,
        role: &str,
        req: &CreateAudioUploadRequest,
    ) -> Result<AudioUploadInitResponse, CliError> {
        let response: AudioUploadInitResponse = self
            .post_voice_json_once(
                "/api/uploads/audio/",
                req,
                workflow_id,
                &format!("{role}_upload_create"),
                &[("recording_role", Value::String(role.to_string()))],
            )
            .await?;
        if response.id.trim().is_empty() || response.url.trim().is_empty() {
            return Err(ambiguous_voice_write(
                workflow_id,
                &format!("{role}_upload_create_response_schema"),
                "schema_drift",
                "Voice upload creation returned a blank id or URL".into(),
                &[("recording_role", Value::String(role.to_string()))],
            ));
        }
        Ok(response)
    }

    /// Finish a Voice upload without replaying the write after an auth error.
    pub(crate) async fn finish_voice_recording_upload(
        &self,
        workflow_id: &str,
        role: &str,
        upload_id: &str,
        req: &FinishAudioUploadRequest,
    ) -> Result<(), CliError> {
        self.post_voice_empty_once(
            &format!("/api/uploads/audio/{upload_id}/upload-finish/"),
            req,
            workflow_id,
            &format!("{role}_upload_finish"),
            &[
                ("recording_role", Value::String(role.to_string())),
                ("upload_id", Value::String(upload_id.to_string())),
            ],
        )
        .await
    }

    pub(crate) async fn process_voice_sample(
        &self,
        workflow_id: &str,
        req: &ProcessVoiceSampleRequest,
    ) -> Result<ProcessVoiceSampleResponse, CliError> {
        let response: ProcessVoiceSampleResponse = self
            .post_voice_json_once(
                "/api/processed_clip/voice-vox-stem",
                req,
                workflow_id,
                "sample_process",
                &[("upload_id", Value::String(req.upload_id.clone()))],
            )
            .await?;
        if response.id.trim().is_empty() || response.voice_recording_id.trim().is_empty() {
            return Err(ambiguous_voice_write(
                workflow_id,
                "sample_process_response_schema",
                "schema_drift",
                "voice-vox-stem returned a blank processed or voice recording id".into(),
                &[("upload_id", Value::String(req.upload_id.clone()))],
            ));
        }
        Ok(response)
    }

    pub(crate) async fn process_voice_verification_recording(
        &self,
        workflow_id: &str,
        req: &ProcessVoiceVerificationRecordingRequest,
    ) -> Result<ProcessVoiceVerificationRecordingResponse, CliError> {
        let response: ProcessVoiceVerificationRecordingResponse = self
            .post_voice_json_once(
                "/api/processed_clip/voice-vox-stem",
                req,
                workflow_id,
                "verification_recording_process",
                &[("upload_id", Value::String(req.upload_id.clone()))],
            )
            .await?;
        if response.voice_recording_id.trim().is_empty() {
            return Err(ambiguous_voice_write(
                workflow_id,
                "verification_recording_process_response_schema",
                "schema_drift",
                "voice-vox-stem returned a blank verification recording id".into(),
                &[("upload_id", Value::String(req.upload_id.clone()))],
            ));
        }
        Ok(response)
    }

    pub(crate) async fn create_voice_verification(
        &self,
        workflow_id: &str,
        req: &CreateVoiceVerificationRequest,
    ) -> Result<VoiceVerification, CliError> {
        let response: VoiceVerification = self
            .post_voice_json_once(
                "/api/voice-verification/",
                req,
                workflow_id,
                "verification_create",
                &[
                    (
                        "voice_recording_id",
                        Value::String(req.voice_recording_id.clone()),
                    ),
                    (
                        "verification_recording_id",
                        Value::String(req.verification_recording_id.clone()),
                    ),
                    ("phrase_id", Value::String(req.phrase_id.clone())),
                ],
            )
            .await?;
        if response.id.trim().is_empty() || response.status.trim().is_empty() {
            return Err(ambiguous_voice_write(
                workflow_id,
                "verification_create_response_schema",
                "schema_drift",
                "Voice verification returned a blank id or status".into(),
                &[
                    (
                        "voice_recording_id",
                        Value::String(req.voice_recording_id.clone()),
                    ),
                    (
                        "verification_recording_id",
                        Value::String(req.verification_recording_id.clone()),
                    ),
                    ("phrase_id", Value::String(req.phrase_id.clone())),
                ],
            ));
        }
        Ok(response)
    }

    pub(crate) async fn create_verified_voice_persona(
        &self,
        workflow_id: &str,
        req: &CreatePersonaRequest,
    ) -> Result<PersonaInfo, CliError> {
        let response: PersonaInfo = match self
            .post_voice_json_once(
                "/api/persona/create/",
                req,
                workflow_id,
                "persona_create",
                &[],
            )
            .await
        {
            Err(error @ CliError::SunoApi { status: 409, .. }) => {
                return Err(ambiguous_voice_persona_conflict(
                    workflow_id,
                    req,
                    error.to_string(),
                ));
            }
            result => result?,
        };
        if response.id.trim().is_empty() {
            return Err(ambiguous_voice_write(
                workflow_id,
                "persona_create_response_schema",
                "schema_drift",
                "Voice persona creation returned a blank persona id".into(),
                &[],
            ));
        }
        Ok(response)
    }

    async fn post_voice_json_once<Req, Resp>(
        &self,
        path: &str,
        req: &Req,
        workflow_id: &str,
        stage: &str,
        context: &[(&str, Value)],
    ) -> Result<Resp, CliError>
    where
        Req: Serialize + ?Sized,
        Resp: DeserializeOwned,
    {
        let request = self.post_without_redirect(path).json(req);
        let response = self
            .prepare_mutation_request(request)
            .await?
            .send()
            .await
            .map_err(|error| {
                ambiguous_voice_write(
                    workflow_id,
                    &format!("{stage}_request_send"),
                    "http_error",
                    error.to_string(),
                    context,
                )
            })?;
        if response.status().is_redirection() || response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_voice_write(
                workflow_id,
                &format!("{stage}_response_status"),
                "http_error",
                format!("HTTP {status}: {body}"),
                context,
            ));
        }
        let response = if path == "/api/persona/create/" {
            // A conflict can mean a prior accepted Voice workflow already created
            // the persona, so preserve its possibly-sent recovery evidence.
            self.check_response_preserving_conflict(response).await?
        } else {
            self.check_response(response).await?
        };
        let body = response.bytes().await.map_err(|error| {
            ambiguous_voice_write(
                workflow_id,
                &format!("{stage}_response_body"),
                "http_error",
                error.to_string(),
                context,
            )
        })?;
        let raw: Value = serde_json::from_slice(&body).map_err(|error| {
            ambiguous_voice_write(
                workflow_id,
                &format!("{stage}_response_schema"),
                "json_error",
                error.to_string(),
                context,
            )
        })?;
        crate::core::operation::record_response(path, &raw).map_err(|error| {
            ambiguous_voice_write(
                workflow_id,
                &format!("{stage}_checkpoint_persist"),
                error.error_code(),
                error.to_string(),
                context,
            )
        })?;
        serde_json::from_value(raw).map_err(|error| {
            ambiguous_voice_write(
                workflow_id,
                &format!("{stage}_response_schema"),
                "json_error",
                error.to_string(),
                context,
            )
        })
    }

    async fn post_voice_empty_once<Req>(
        &self,
        path: &str,
        req: &Req,
        workflow_id: &str,
        stage: &str,
        context: &[(&str, Value)],
    ) -> Result<(), CliError>
    where
        Req: Serialize + ?Sized,
    {
        let request = self.post_without_redirect(path).json(req);
        let response = self
            .prepare_mutation_request(request)
            .await?
            .send()
            .await
            .map_err(|error| {
                ambiguous_voice_write(
                    workflow_id,
                    &format!("{stage}_request_send"),
                    "http_error",
                    error.to_string(),
                    context,
                )
            })?;
        if response.status().is_redirection() || response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_voice_write(
                workflow_id,
                &format!("{stage}_response_status"),
                "http_error",
                format!("HTTP {status}: {body}"),
                context,
            ));
        }
        self.check_response(response).await?;
        Ok(())
    }
}

fn ambiguous_voice_write(
    workflow_id: &str,
    stage: &str,
    cause_code: &str,
    cause_message: String,
    context: &[(&str, Value)],
) -> CliError {
    let mut ambiguity = MutationAmbiguity::new(
        format!(
            "Voice workflow {workflow_id} lost a reliable response during {stage}; Suno may have accepted the write"
        ),
        "voice_create",
        workflow_id,
        stage,
        cause_code,
        cause_message,
        false,
        "Voice creation POSTs have no verified idempotency key; replay may duplicate uploads, verification records, or personas",
        vec![
            "sunox voice processed-status <processed_id> --json".into(),
            "sunox voice verification-status <verification_id> --json".into(),
            "sunox persona list --json".into(),
        ],
    );
    for (key, value) in context {
        ambiguity = ambiguity.with_context(*key, value.clone());
    }
    ambiguity.into_error()
}

fn identity_mismatch(resource: &str, requested_id: &str, actual_id: &str) -> CliError {
    CliError::Api {
        code: "schema_drift",
        message: format!("Suno returned {resource} `{actual_id}` while resolving `{requested_id}`"),
    }
}

fn ambiguous_voice_persona_conflict(
    workflow_id: &str,
    req: &CreatePersonaRequest,
    cause_message: String,
) -> CliError {
    let mut ambiguity = MutationAmbiguity::new(
        format!(
            "Voice workflow {workflow_id} received HTTP 409 while creating the Persona; an earlier accepted request may already have created it"
        ),
        "voice_create",
        workflow_id,
        "persona_create_conflict",
        "already_exists",
        cause_message,
        false,
        "do not replay Persona creation; inspect the Persona list for a private vox Voice matching the checkpointed voice and verification identities",
        vec!["sunox persona list --json".into()],
    );
    if let Some(value) = &req.voice_recording_id {
        ambiguity = ambiguity.with_context("voice_recording_id", Value::String(value.clone()));
    }
    if let Some(value) = &req.verification_id {
        ambiguity = ambiguity.with_context("verification_id", Value::String(value.clone()));
    }
    if let Some(value) = &req.vox_audio_id {
        ambiguity = ambiguity.with_context("vox_audio_id", Value::String(value.clone()));
    }
    ambiguity.into_error()
}
