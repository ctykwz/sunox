use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;

use super::types::{
    ArchiveCustomModelRequest, BillingInfo, CreateCustomModelRequest, CustomModelCreateResponse,
    PendingCustomModelsResponse,
};
use super::{PollingOptions, SunoClient};
use crate::core::{CliError, MutationAmbiguity, run_before_deadline, sleep_before_deadline};

const CUSTOM_MODEL_READBACK: PollingOptions = PollingOptions {
    timeout: Duration::from_secs(45),
    interval: Duration::from_secs(15),
};

impl SunoClient {
    pub async fn pending_custom_models(&self) -> Result<PendingCustomModelsResponse, CliError> {
        self.with_auth_retry(|| async {
            let raw: Value = self
                .read_json_with_transport_retry(self.get("/api/custom-model/pending/"))
                .await?;
            serde_json::from_value(raw).map_err(|error| CliError::Api {
                code: "schema_drift",
                message: format!("invalid Custom Model pending response: {error}"),
            })
        })
        .await
    }

    pub async fn create_custom_model(
        &self,
        clip_ids: &[String],
        name: &str,
        confirm_ui_available: bool,
    ) -> Result<CustomModelCreateResponse, CliError> {
        validate_custom_model_create(clip_ids, name)?;
        let billing = self.billing_info().await?;
        if !billing
            .accessible_features
            .as_ref()
            .is_some_and(|features| features.contains("custom_models"))
        {
            return Err(CliError::Config(
                "Suno does not expose `custom_models` in the current account's accessible_features"
                    .into(),
            ));
        }
        if !confirm_ui_available {
            return Err(CliError::Config(
                "Suno Web currently also requires `custom-model-ui`; pass --confirm-ui-available only after confirming that the current account visibly exposes Custom Model training. The server remains authoritative for eligibility and charging"
                    .into(),
            ));
        }
        self.create_custom_model_after_ui_gate(clip_ids, name, CUSTOM_MODEL_READBACK)
            .await
    }

    /// Submit only after the public entry point has checked the live account
    /// entitlement and received the caller's explicit Web-UI visibility
    /// attestation. This stays inside the API module so normal CLI code cannot
    /// bypass those preflights while no API read seam exposes the UI gate.
    pub(super) async fn create_custom_model_after_ui_gate(
        &self,
        clip_ids: &[String],
        name: &str,
        polling: PollingOptions,
    ) -> Result<CustomModelCreateResponse, CliError> {
        validate_custom_model_create(clip_ids, name)?;
        let authenticated_user_id = self.authenticated_user_id().ok_or_else(|| {
            CliError::Config(
                "the authenticated account identity is not present in the current JWT; refusing Custom Model training"
                    .into(),
            )
        })?;
        for clip_id in clip_ids {
            let clip = self
                .get_clip(clip_id)
                .await?
                .ok_or_else(|| CliError::NotFound(format!("clip {clip_id}")))?;
            if clip.id != *clip_id {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "Custom Model source lookup for `{clip_id}` returned clip `{}`",
                        clip.id
                    ),
                });
            }
            let source_user_id = clip.extra.get("user_id").and_then(Value::as_str);
            if source_user_id != Some(authenticated_user_id.as_str()) {
                return Err(CliError::Config(format!(
                    "Custom Model source clip `{clip_id}` must be explicitly owned by the authenticated account"
                )));
            }
            if clip.status != "complete" {
                return Err(CliError::Config(format!(
                    "Custom Model source clip `{clip_id}` must be complete"
                )));
            }
            if clip.is_trashed != Some(false) {
                return Err(CliError::Config(format!(
                    "Custom Model source clip `{clip_id}` must be explicitly non-trashed"
                )));
            }
        }

        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = CreateCustomModelRequest { clip_ids, name };
        let response = self
            .post_without_redirect("/api/custom-model/create/")
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                ambiguous_custom_model_create(
                    &operation_id,
                    clip_ids,
                    name,
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        if response.status().is_redirection() || response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_custom_model_create(
                &operation_id,
                clip_ids,
                name,
                "response_status",
                "http_error",
                format!("HTTP {status}: {body}"),
            ));
        }
        let response = self.check_response(response).await?;
        let raw: Value = response.json().await.map_err(|error| {
            ambiguous_custom_model_create(
                &operation_id,
                clip_ids,
                name,
                "response_body",
                "http_error",
                error.to_string(),
            )
        })?;
        let created: CustomModelCreateResponse = serde_json::from_value(raw).map_err(|error| {
            ambiguous_custom_model_create(
                &operation_id,
                clip_ids,
                name,
                "response_schema",
                "schema_drift",
                error.to_string(),
            )
        })?;
        self.confirm_custom_model_create_readback(&operation_id, &created.id, polling)
            .await?;
        Ok(created)
    }

    async fn confirm_custom_model_create_readback(
        &self,
        operation_id: &str,
        model_id: &str,
        polling: PollingOptions,
    ) -> Result<(), CliError> {
        let deadline = polling.deadline()?;
        let (pending, billing) = loop {
            let pending = run_before_deadline(
                deadline,
                self.pending_custom_models(),
                custom_model_readback_timeout(model_id),
            )
            .await;
            let billing = run_before_deadline(
                deadline,
                self.billing_info(),
                custom_model_readback_timeout(model_id),
            )
            .await;
            let observed = pending
                .as_ref()
                .ok()
                .is_some_and(|models| pending_contains(model_id, models))
                || billing
                    .as_ref()
                    .ok()
                    .is_some_and(|info| billing_contains_custom_model(model_id, info));
            if observed {
                return Ok(());
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                break (pending, billing);
            }
        };

        Err(custom_create_readback_error(
            operation_id,
            model_id,
            pending.as_ref().err(),
            billing.as_ref().err(),
        ))
    }

    pub async fn archive_custom_model(&self, model_id: &str) -> Result<(), CliError> {
        self.archive_custom_model_with_readback(model_id, CUSTOM_MODEL_READBACK)
            .await
    }

    pub(super) async fn archive_custom_model_with_readback(
        &self,
        model_id: &str,
        polling: PollingOptions,
    ) -> Result<(), CliError> {
        let pending_before = self.pending_custom_models().await?;
        let billing_before = self.billing_info().await?;
        if !custom_model_exists(model_id, &pending_before, &billing_before) {
            return Err(CliError::NotFound(format!("Custom Model {model_id}")));
        }

        let operation_id = uuid::Uuid::new_v4().to_string();
        let response = self
            .post_without_redirect("/api/custom-model/archive/")
            .json(&ArchiveCustomModelRequest { id: model_id })
            .send()
            .await
            .map_err(|error| {
                ambiguous_custom_model_archive(
                    &operation_id,
                    model_id,
                    "request_send",
                    error.to_string(),
                )
            })?;
        if response.status().is_redirection() || response.status().is_server_error() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ambiguous_custom_model_archive(
                &operation_id,
                model_id,
                "response_status",
                format!("HTTP {status}: {body}"),
            ));
        }
        self.check_response(response).await?;

        self.confirm_custom_model_archive_readback(&operation_id, model_id, polling)
            .await
    }

    async fn confirm_custom_model_archive_readback(
        &self,
        operation_id: &str,
        model_id: &str,
        polling: PollingOptions,
    ) -> Result<(), CliError> {
        let deadline = polling.deadline()?;
        let (pending, billing) = loop {
            let pending = run_before_deadline(
                deadline,
                self.pending_custom_models(),
                custom_model_readback_timeout(model_id),
            )
            .await;
            let billing = run_before_deadline(
                deadline,
                self.billing_info(),
                custom_model_readback_timeout(model_id),
            )
            .await;
            if let (Ok(pending), Ok(billing)) = (&pending, &billing)
                && !custom_model_exists(model_id, pending, billing)
            {
                return Ok(());
            }
            if !sleep_before_deadline(deadline, polling.interval).await {
                break (pending, billing);
            }
        };

        let (failed_step, error) = match (pending, billing) {
            (Err(error), _) => ("pending_readback", error),
            (_, Err(error)) => ("billing_readback", error),
            (Ok(_), Ok(_)) => (
                "state_mismatch",
                CliError::Api {
                    code: "readback_mismatch",
                    message: "Custom Model remains visible after archive was accepted".into(),
                },
            ),
        };
        Err(custom_archive_readback_error(
            operation_id,
            model_id,
            failed_step,
            error,
        ))
    }
}

fn validate_custom_model_create(clip_ids: &[String], name: &str) -> Result<(), CliError> {
    if name.trim().is_empty() {
        return Err(CliError::Config(
            "Custom Model name must not be empty".into(),
        ));
    }
    if name.trim().chars().count() > 16 {
        return Err(CliError::Config(
            "Custom Model name must not exceed the current Web limit of 16 Unicode characters"
                .into(),
        ));
    }
    let mut unique = BTreeSet::new();
    for clip_id in clip_ids {
        let clip_id = clip_id.trim();
        if clip_id.is_empty() {
            return Err(CliError::Config(
                "Custom Model source clip ID must not be empty".into(),
            ));
        }
        if !unique.insert(clip_id) {
            return Err(CliError::Config(format!(
                "Custom Model source clip `{clip_id}` is duplicated; provide at least 6 distinct clip IDs"
            )));
        }
    }
    if unique.len() < 6 {
        return Err(CliError::Config(
            "Custom Model training requires at least 6 distinct clip IDs".into(),
        ));
    }
    if unique.len() > 100 {
        return Err(CliError::Config(
            "Custom Model training accepts at most 100 source clips through this API seam".into(),
        ));
    }
    Ok(())
}

fn custom_model_exists(
    model_id: &str,
    pending: &PendingCustomModelsResponse,
    billing: &BillingInfo,
) -> bool {
    pending_contains(model_id, pending) || billing_contains_custom_model(model_id, billing)
}

fn pending_contains(model_id: &str, pending: &PendingCustomModelsResponse) -> bool {
    pending
        .pending_models
        .iter()
        .any(|model| model.id == model_id)
}

fn billing_contains_custom_model(model_id: &str, billing: &BillingInfo) -> bool {
    billing.models.iter().any(|model| {
        // `custom` on a base-model row means "supports custom prompting" in
        // the current Web model helper. Require the exact archive-modal ID and
        // the observed Custom Model external-key family as well as its badge.
        let has_custom_model_badge = model
            .badges
            .iter()
            .any(|badge| matches!(badge.as_str(), "custom" | "training"));
        let has_custom_model_external_key = model.external_key.starts_with("chirp-custom");
        has_custom_model_badge
            && has_custom_model_external_key
            && model.extra.get("id").and_then(Value::as_str) == Some(model_id)
    })
}

fn custom_create_readback_error(
    operation_id: &str,
    model_id: &str,
    pending_error: Option<&CliError>,
    billing_error: Option<&CliError>,
) -> CliError {
    let pending = pending_error.map_or_else(
        || serde_json::json!({"observed": false}),
        |error| {
            serde_json::json!({
                "observed": false,
                "error": {"code": error.error_code(), "message": error.to_string()}
            })
        },
    );
    let billing = billing_error.map_or_else(
        || serde_json::json!({"observed": false}),
        |error| {
            serde_json::json!({
                "observed": false,
                "error": {"code": error.error_code(), "message": error.to_string()}
            })
        },
    );
    CliError::PartialMutation {
        message: format!(
            "Custom Model training {operation_id} returned model {model_id}, but pending/billing readback did not confirm it"
        ),
        details: serde_json::json!({
            "operation": "custom_model_create",
            "operation_id": operation_id,
            "model_id": model_id,
            "completed_steps": ["training_accepted"],
            "readback": {"pending": pending, "billing": billing},
            "recovery": {
                "resumable": true,
                "reason": "resume only read-only pending/billing inspection; eventual consistency is possible and create is not safe to replay",
                "inspection_commands": [
                    "sunox models custom pending --json",
                    "sunox models --json",
                    "sunox credits --json"
                ]
            }
        }),
    }
}

fn custom_archive_readback_error(
    operation_id: &str,
    model_id: &str,
    failed_step: &'static str,
    error: CliError,
) -> CliError {
    CliError::PartialMutation {
        message: format!(
            "Custom Model archive for {model_id} was accepted but failed business readback at {failed_step}"
        ),
        details: serde_json::json!({
            "operation": "custom_model_archive",
            "operation_id": operation_id,
            "model_id": model_id,
            "completed_steps": ["archive_accepted"],
            "failed": {
                "step": failed_step,
                "code": error.error_code(),
                "message": error.to_string(),
            },
            "recovery": {
                "resumable": true,
                "reason": "resume only the read-only state inspection; do not replay archive until absence is proven",
                "inspection_commands": [
                    "sunox models custom pending --json",
                    "sunox models --json"
                ]
            }
        }),
    }
}

fn custom_model_readback_timeout(model_id: &str) -> CliError {
    CliError::Api {
        code: "readback_timeout",
        message: format!("timed out reading Custom Model {model_id} state"),
    }
}

fn ambiguous_custom_model_create(
    operation_id: &str,
    clip_ids: &[String],
    name: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "Custom Model training operation {operation_id} lost a reliable response during {stage}; training may already have started"
        ),
        "custom_model_create",
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "the create endpoint has no captured idempotency key, so replay may duplicate training or credit usage",
        vec![
            "sunox models custom pending --json".into(),
            "sunox models --json".into(),
            "sunox credits --json".into(),
        ],
    )
    .with_context("name", Value::String(name.to_string()))
    .with_context("clip_ids", serde_json::json!(clip_ids))
    .into_error()
}

fn ambiguous_custom_model_archive(
    operation_id: &str,
    model_id: &str,
    stage: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "Custom Model archive {operation_id} lost a reliable response during {stage}; model {model_id} may already be archived"
        ),
        "custom_model_archive",
        operation_id,
        stage,
        "http_error",
        cause_message,
        false,
        "the archive endpoint has no captured idempotency key; inspect active and pending Custom Models before any replay",
        vec![
            "sunox models --json".into(),
            "sunox models custom pending --json".into(),
        ],
    )
    .with_context("model_id", Value::String(model_id.to_string()))
    .into_error()
}
