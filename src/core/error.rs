use thiserror::Error;

#[derive(Error, Debug)]
pub enum CliError {
    #[error("API error: {message}")]
    Api { code: &'static str, message: String },

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

    #[error("Authentication required — run `sunox login` first")]
    AuthMissing,

    #[error("JWT expired or rejected by Suno")]
    AuthExpired,

    #[error("Rate limited by Suno — wait and retry")]
    RateLimited,

    #[error("Generation failed: {0}")]
    GenerationFailed(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Download failed: {0}")]
    Download(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Self-update failed: {0}")]
    Update(String),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Config(_) => 2,
            Self::AuthMissing | Self::AuthExpired => 3,
            Self::RateLimited => 4,
            Self::NotFound(_) => 5,
            Self::Api { .. }
            | Self::PartialMutation { .. }
            | Self::PartialDownload { .. }
            | Self::Http(_)
            | Self::GenerationFailed(_)
            | Self::Download(_)
            | Self::Update(_)
            | Self::Io(_)
            | Self::Json(_) => 1,
        }
    }

    pub fn error_code(&self) -> &'static str {
        match self {
            Self::Api { code, .. } => code,
            Self::PartialMutation { .. } => "partial_mutation",
            Self::PartialDownload { .. } => "partial_download",
            Self::AuthMissing => "auth_missing",
            Self::AuthExpired => "auth_expired",
            Self::RateLimited => "rate_limited",
            Self::Config(_) => "config_error",
            Self::GenerationFailed(_) => "generation_failed",
            Self::Download(_) => "download_error",
            Self::NotFound(_) => "not_found",
            Self::Http(_) => "http_error",
            Self::Io(_) => "io_error",
            Self::Json(_) => "json_error",
            Self::Update(_) => "update_error",
        }
    }

    pub fn suggestion(&self) -> &'static str {
        match self {
            Self::AuthMissing => "Run `sunox login` to authenticate",
            Self::AuthExpired => "Run `sunox auth --refresh`; if that fails, run `sunox login`",
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
            Self::PartialMutation { .. } => {
                "Inspect error.details before retrying; when recovery is present, follow it only if recovery.resumable is true"
            }
            Self::PartialDownload { .. } => {
                "Inspect error.details for succeeded paths, the failed clip, and not_attempted IDs before retrying"
            }
            Self::Api { code, .. } if *code == "schema_drift" => {
                "Suno changed its web schema or challenge enforcement. Try (1) `sunox auth --refresh` to mint a fresh JWT, (2) `sunox update` to pull the latest fix, (3) supply a challenge token via `--token <solved>`, or (4) see https://github.com/ctykwz/sunox/issues for the current status"
            }
            Self::Api { .. } | Self::Http(_) => "Check your network connection and retry",
            Self::Io(_) => "Check file permissions and disk space",
            Self::Json(_) => {
                "This may indicate a response schema change — run `sunox update` for the latest fix"
            }
            Self::Update(_) => {
                "Check your network connection or download the binary directly from GitHub Releases"
            }
        }
    }

    pub fn details(&self) -> Option<&serde_json::Value> {
        match self {
            Self::PartialMutation { details, .. } | Self::PartialDownload { details, .. } => {
                Some(details)
            }
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
}
