use thiserror::Error;

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

    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_) => 2,
            Self::AuthMissing | Self::AuthExpired | Self::AuthChanged => 3,
            Self::RateLimited => 4,
            Self::NotFound(_) | Self::SunoApi { status: 404, .. } => 5,
            Self::Interrupted => 130,
            Self::Api { .. }
            | Self::SunoApi { .. }
            | Self::PartialMutation { .. }
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
            Self::PartialDownload { .. } => {
                "Inspect error.details for succeeded paths, the failed clip, and not_attempted IDs before retrying"
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
            Self::Interrupted => "The operation was cancelled and temporary files were cleaned up",
        }
    }

    pub fn details(&self) -> Option<&serde_json::Value> {
        match self {
            Self::PartialMutation { details, .. }
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
    use super::CliError;

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
}
