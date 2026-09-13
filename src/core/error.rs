use thiserror::Error;

#[derive(Debug)]
pub(crate) struct MutationAmbiguity {
    message: String,
    operation: String,
    operation_id: String,
    stage: String,
    cause_code: String,
    cause_message: String,
    resumable: bool,
    recovery_reason: String,
    inspection_commands: Vec<String>,
    context: serde_json::Map<String, serde_json::Value>,
}

impl MutationAmbiguity {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        message: impl Into<String>,
        operation: impl Into<String>,
        operation_id: impl Into<String>,
        stage: impl Into<String>,
        cause_code: impl Into<String>,
        cause_message: impl Into<String>,
        resumable: bool,
        recovery_reason: impl Into<String>,
        inspection_commands: Vec<String>,
    ) -> Self {
        Self {
            message: message.into(),
            operation: operation.into(),
            operation_id: operation_id.into(),
            stage: stage.into(),
            cause_code: cause_code.into(),
            cause_message: cause_message.into(),
            resumable,
            recovery_reason: recovery_reason.into(),
            inspection_commands,
            context: serde_json::Map::new(),
        }
    }

    pub(crate) fn with_context(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.context.insert(key.into(), value);
        self
    }

    pub(crate) fn into_error(self) -> CliError {
        let mut details = self.context;
        details.insert(
            "operation".into(),
            serde_json::Value::String(self.operation),
        );
        details.insert(
            "operation_id".into(),
            serde_json::Value::String(self.operation_id),
        );
        details.insert("stage".into(), serde_json::Value::String(self.stage));
        details.insert(
            "cause".into(),
            serde_json::json!({
                "code": self.cause_code,
                "message": self.cause_message,
            }),
        );
        details.insert(
            "recovery".into(),
            serde_json::json!({
                "resumable": self.resumable,
                "reason": self.recovery_reason,
                "inspection_commands": self.inspection_commands,
            }),
        );
        CliError::AmbiguousMutation {
            message: self.message,
            details: serde_json::Value::Object(details),
        }
    }
}

#[derive(Error, Debug)]
pub enum CliError {
    #[error("API error: {message}")]
    Api { code: &'static str, message: String },

    #[error("API error: {message}")]
    SunoApi {
        code: &'static str,
        status: u16,
        message: String,
        retryable: Option<bool>,
        details: Option<serde_json::Value>,
    },

    #[error("Partial mutation failure: {message}")]
    PartialMutation {
        message: String,
        details: serde_json::Value,
    },

    #[error("Mutation result is ambiguous: {message}")]
    AmbiguousMutation {
        message: String,
        details: serde_json::Value,
    },

    #[error("Partial download failure: {message}")]
    PartialDownload {
        message: String,
        details: serde_json::Value,
    },

    #[error("Diagnostic failed: {message}")]
    Diagnostic {
        code: &'static str,
        message: String,
        details: serde_json::Value,
    },

    #[error("Authentication required — run `sunox login` first")]
    AuthMissing,

    #[error("JWT expired or rejected by Suno")]
    AuthExpired,

    #[error("Active Suno authentication changed while the command was in progress")]
    AuthChanged,

    #[error("Rate limited by Suno — wait and retry")]
    RateLimited,

    #[error("Generation failed: {0}")]
    GenerationFailed(String),

    #[error("Generation challenge required: {0}")]
    ChallengeRequired(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Download failed: {0}")]
    Download(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Self-update failed: {0}")]
    Update(String),

    #[error("Interrupted by user")]
    Interrupted,

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl CliError {
    pub(crate) fn is_auth_or_rate_limit(&self) -> bool {
        matches!(
            self,
            Self::AuthMissing | Self::AuthExpired | Self::AuthChanged | Self::RateLimited
        )
    }

    pub(crate) fn stops_account_work(&self) -> bool {
        self.is_auth_or_rate_limit()
            || matches!(self, Self::AmbiguousMutation { details, .. }
                if matches!(details.pointer("/cause/code").and_then(serde_json::Value::as_str),
                    Some("auth_missing" | "auth_expired" | "auth_changed" | "rate_limited")))
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_)
            | Self::Diagnostic {
                code: "config_error",
                ..
            } => 2,
            Self::AuthMissing | Self::AuthExpired | Self::AuthChanged => 3,
            Self::RateLimited => 4,
            Self::NotFound(_) | Self::SunoApi { status: 404, .. } => 5,
            Self::Interrupted => 130,
            Self::Api { .. }
            | Self::SunoApi { .. }
            | Self::PartialMutation { .. }
            | Self::AmbiguousMutation { .. }
            | Self::PartialDownload { .. }
            | Self::Diagnostic { .. }
            | Self::Http(_)
            | Self::GenerationFailed(_)
            | Self::ChallengeRequired(_)
            | Self::Download(_)
            | Self::Update(_)
            | Self::Io(_)
            | Self::Json(_) => 1,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Api { code, .. } => code,
            Self::SunoApi { code, .. } => code,
            Self::PartialMutation { .. } => "partial_mutation",
            Self::AmbiguousMutation { .. } => "ambiguous_mutation",
            Self::PartialDownload { .. } => "partial_download",
            Self::Diagnostic { code, .. } => code,
            Self::AuthMissing => "auth_missing",
            Self::AuthExpired => "auth_expired",
            Self::AuthChanged => "auth_changed",
            Self::RateLimited => "rate_limited",
            Self::Config(_) => "config_error",
            Self::GenerationFailed(_) => "generation_failed",
            Self::ChallengeRequired(_) => "challenge_required",
            Self::Download(_) => "download_error",
            Self::NotFound(_) => "not_found",
            Self::Http(_) => "http_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "json_error",
            Self::Update(_) => "update_error",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn suggestion(&self) -> &'static str {
        match self {
            Self::AuthMissing => "Run `sunox login` to authenticate",
            Self::AuthExpired => "Run `sunox auth --refresh`; if that fails, run `sunox login`",
            Self::AuthChanged => {
                "Retry the command with the current login; run `sunox auth` if the account switch was unintended"
            }
            Self::RateLimited => "Wait 30-60 seconds and retry",
            Self::Config(_) => "Check `sunox doctor` for configuration issues",
            Self::NotFound(_) => {
                "Verify the ID exists with `sunox clip list` or `sunox clip search`"
            }
            Self::Download(_) => {
                "Check that the clip has finished generating with `sunox clip status <id>`"
            }
            Self::GenerationFailed(_) => {
                "Inspect the failure message and retry only after addressing the reported cause"
            }
            Self::ChallengeRequired(_) => {
                "Keep a supported local browser available and retry without `--no-captcha`; otherwise provide a valid challenge token with `--token` or complete a manual generation challenge in the Suno web app"
            }
            Self::PartialMutation { .. } => {
                "Inspect error.details before retrying; when recovery is present, follow it only if recovery.resumable is true"
            }
            Self::AmbiguousMutation { .. } => {
                "Do not blindly retry: Suno may have accepted the write. Inspect error.details and run its read-only inspection commands first"
            }
            Self::PartialDownload { .. } => {
                "Inspect error.details for succeeded paths, authorized_sources, the failed clip, and not_attempted IDs before retrying"
            }
            Self::Diagnostic {
                code: "config_error",
                ..
            } => {
                "Inspect error.details.config.path; use `sunox config set <key> <value>` to repair a field, or correct invalid TOML syntax in that file"
            }
            Self::Diagnostic {
                code: "download_authorization_required",
                ..
            } => {
                "Inspect the source clip and live download usage; remove --read-only only when consuming download allowance is intentional"
            }
            Self::Diagnostic {
                code: "download_authorization_denied",
                ..
            } => {
                "Inspect error.details.reason and `sunox credits --json`; do not retry until the reported account or allowance condition changes"
            }
            Self::Diagnostic {
                code: "prepared_download_unavailable",
                ..
            } => {
                "Verify the clip is complete and unlocked; retry only after Suno reports the requested prepared format as available"
            }
            Self::Diagnostic { .. } => {
                "Inspect error.details for the failed diagnostic stages and correct the reported environment problem"
            }
            Self::Api { code, .. } if *code == "schema_drift" => {
                "Suno changed its web schema or challenge enforcement. Try (1) `sunox auth --refresh` to mint a fresh JWT, (2) `sunox update` to pull the latest fix, (3) supply a challenge token via `--token <solved>`, or (4) see https://github.com/ctykwz/sunox/issues for the current status"
            }
            Self::SunoApi { code, .. } if *code == "schema_drift" => {
                "Suno changed its web schema. Run `sunox update`; if the error remains, report the response details"
            }
            Self::SunoApi {
                retryable: Some(false),
                ..
            } => {
                "Do not retry the same request unchanged; inspect error.details and correct the request or resource state"
            }
            Self::SunoApi {
                retryable: Some(true),
                ..
            } => "Suno marked this failure as retryable; wait before retrying",
            Self::SunoApi { status: 404, .. } => {
                "Verify the resource ID and whether this Suno web route is still available"
            }
            Self::SunoApi { status, .. } if (400..500).contains(status) => {
                "Inspect error.details and correct the request before retrying"
            }
            Self::SunoApi { status, .. } if *status >= 500 => {
                "Suno returned a server error with unknown retryability; inspect error.details before deciding whether to retry"
            }
            Self::SunoApi { .. } => "Inspect error.details before retrying",
            Self::Api { .. } => {
                "Inspect the Suno error response and retry only when it explicitly indicates the request is retryable"
            }
            Self::Http(_) => "Check your network connection and retry",
            Self::Io(_) => "Check file permissions and disk space",
            Self::Json(_) => {
                "This may indicate a response schema change — run `sunox update` for the latest fix"
            }
            Self::Update(_) => {
                "Check your network connection or download the binary directly from GitHub Releases"
            }
            Self::Interrupted => {
                "The CLI stopped; Suno may still complete writes already sent. Inspect any operation_recovery details before submitting again"
            }
        }
    }

    pub fn details(&self) -> Option<&serde_json::Value> {
        match self {
            Self::PartialMutation { details, .. }
            | Self::AmbiguousMutation { details, .. }
            | Self::PartialDownload { details, .. }
            | Self::Diagnostic { details, .. }
            | Self::SunoApi {
                details: Some(details),
                ..
            } => Some(details),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CliError, MutationAmbiguity};

    #[test]
    fn partial_download_exposes_machine_readable_details() {
        let details = serde_json::json!({"succeeded": []});
        let error = CliError::PartialDownload {
            message: "one download failed".into(),
            details: details.clone(),
        };

        assert_eq!(error.error_code(), "partial_download");
        assert_eq!(error.details(), Some(&details));
    }

    #[test]
    fn generation_failure_suggestion_does_not_assume_a_credit_problem() {
        let error = CliError::GenerationFailed("timed out waiting for edit action".into());

        assert!(!error.suggestion().to_ascii_lowercase().contains("credit"));
        assert!(error.suggestion().contains("failure message"));
    }

    #[test]
    fn ambiguous_mutation_exposes_details_and_forbids_blind_retry() {
        let details = serde_json::json!({
            "operation": "generation_submit",
            "operation_id": "transaction-1"
        });
        let error = CliError::AmbiguousMutation {
            message: "generation response was lost".into(),
            details: details.clone(),
        };

        assert_eq!(error.error_code(), "ambiguous_mutation");
        assert_eq!(error.details(), Some(&details));
        assert!(error.suggestion().contains("Do not blindly retry"));
    }

    #[test]
    fn ambiguity_builder_preserves_typed_cause_recovery_and_context() {
        let error = MutationAmbiguity::new(
            "write outcome is unknown",
            "crop",
            "operation-1",
            "response_body",
            "http_error",
            "body ended early",
            false,
            "inspect the clip list before retrying",
            vec!["sunox clip list --json".into()],
        )
        .with_context("source_clip_id", serde_json::json!("clip-a"))
        .into_error();

        let details = error.details().expect("ambiguity details");
        assert_eq!(details["operation"], "crop");
        assert_eq!(details["operation_id"], "operation-1");
        assert_eq!(details["source_clip_id"], "clip-a");
        assert_eq!(details["cause"]["code"], "http_error");
        assert_eq!(details["recovery"]["resumable"], false);
        assert_eq!(
            details["recovery"]["inspection_commands"][0],
            "sunox clip list --json"
        );
    }

    #[test]
    fn suno_api_error_does_not_claim_a_network_failure() {
        let error = CliError::SunoApi {
            code: "api_error",
            status: 500,
            message: "HTTP 500: {\"retryable\":false}".into(),
            retryable: Some(false),
            details: Some(serde_json::json!({"retryable": false})),
        };

        assert!(!error.suggestion().to_ascii_lowercase().contains("network"));
        assert!(error.suggestion().starts_with("Do not retry"));
        assert_eq!(
            error.details(),
            Some(&serde_json::json!({"retryable": false}))
        );
    }

    #[test]
    fn structured_api_not_found_uses_not_found_exit_code() {
        let error = CliError::SunoApi {
            code: "not_found",
            status: 404,
            message: "HTTP 404: missing".into(),
            retryable: Some(false),
            details: None,
        };

        assert_eq!(error.exit_code(), 5);
        assert_eq!(error.error_code(), "not_found");
    }

    #[test]
    fn generation_challenge_has_a_dedicated_machine_contract() {
        let error = CliError::ChallengeRequired("captcha_version=4".into());

        assert_eq!(error.error_code(), "challenge_required");
        assert!(error.suggestion().contains("--no-captcha"));
        assert!(error.suggestion().contains("--token"));
        assert!(!error.suggestion().contains("doctor"));
    }

    #[test]
    fn download_diagnostics_have_protocol_specific_recovery_guidance() {
        for (code, expected) in [
            ("download_authorization_required", "--read-only"),
            ("download_authorization_denied", "credits"),
            ("prepared_download_unavailable", "prepared format"),
        ] {
            let error = CliError::Diagnostic {
                code,
                message: "fixture".into(),
                details: serde_json::json!({}),
            };
            assert!(error.suggestion().contains(expected));
            assert!(!error.suggestion().contains("environment problem"));
        }
    }
}
