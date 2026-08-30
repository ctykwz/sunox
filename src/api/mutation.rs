use serde::de::DeserializeOwned;
use serde_json::Value;

use super::SunoClient;
use crate::core::{CliError, MutationAmbiguity};

pub(crate) struct MutationSpec {
    operation: &'static str,
    operation_id: String,
    resource: String,
    inspection_commands: Vec<String>,
    context: Vec<(&'static str, Value)>,
}

impl MutationSpec {
    pub(crate) fn new(
        operation: &'static str,
        resource: impl Into<String>,
        inspection_commands: Vec<String>,
    ) -> Self {
        Self {
            operation,
            operation_id: uuid::Uuid::new_v4().to_string(),
            resource: resource.into(),
            inspection_commands,
            context: Vec::new(),
        }
    }

    pub(crate) fn with_context(mut self, key: &'static str, value: impl Into<Value>) -> Self {
        self.context.push((key, value.into()));
        self
    }

    pub(crate) fn ambiguous(
        &self,
        stage: &'static str,
        cause_code: &str,
        cause_message: String,
    ) -> CliError {
        let mut ambiguity = MutationAmbiguity::new(
            format!(
                "{} for {} lost a reliable response during {stage}; Suno may have accepted the write",
                self.operation, self.resource
            ),
            self.operation,
            &self.operation_id,
            stage,
            cause_code,
            cause_message,
            false,
            "inspect the exact resource state before retrying; this write has no verified idempotency key",
            self.inspection_commands.clone(),
        )
        .with_context("resource", Value::String(self.resource.clone()));
        for (key, value) in &self.context {
            ambiguity = ambiguity.with_context(*key, value.clone());
        }
        ambiguity.into_error()
    }
}

impl SunoClient {
    pub(crate) async fn prepare_mutation_request(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, CliError> {
        self.refresh_mutation_auth_if_stale().await?;
        Ok(request.headers(self.headers()))
    }

    /// Send a non-idempotent write exactly once. Callers must construct the
    /// request with the no-redirect client. Explicit 4xx responses remain
    /// ordinary API errors; transport loss, redirects, and 5xx responses are
    /// ambiguous because the server may already have committed the write.
    pub(crate) async fn send_mutation_once(
        &self,
        mut request: reqwest::RequestBuilder,
        spec: &MutationSpec,
    ) -> Result<reqwest::Response, CliError> {
        request = self.prepare_mutation_request(request).await?;
        let response = request
            .send()
            .await
            .map_err(|error| spec.ambiguous("request_send", "http_error", error.to_string()))?;
        let status = response.status();
        if status.is_redirection() || status.is_server_error() {
            let body = response.text().await.unwrap_or_default();
            return Err(spec.ambiguous(
                "response_status",
                "http_error",
                format!("HTTP {status}: {body}"),
            ));
        }
        self.check_response(response).await
    }

    pub(crate) async fn mutation_json_once<T>(
        &self,
        request: reqwest::RequestBuilder,
        spec: &MutationSpec,
    ) -> Result<T, CliError>
    where
        T: DeserializeOwned,
    {
        let response = self.send_mutation_once(request, spec).await?;
        response
            .json::<T>()
            .await
            .map_err(|error| spec.ambiguous("response_body", "schema_drift", error.to_string()))
    }

    pub(crate) async fn mutation_text_once(
        &self,
        request: reqwest::RequestBuilder,
        spec: &MutationSpec,
    ) -> Result<String, CliError> {
        let response = self.send_mutation_once(request, spec).await?;
        response
            .text()
            .await
            .map_err(|error| spec.ambiguous("response_body", "http_error", error.to_string()))
    }
}
