//! Suno web endpoint client, endpoint methods, auth retry, and response mapping.

use std::time::Duration;

pub mod billing;
pub mod challenge;
pub mod clip_info;
pub mod concat;
pub mod cover;
pub mod delete;
pub mod download;
pub mod edit;
pub mod extend;
pub mod feed;
pub mod generate;
pub mod lyrics;
pub mod metadata;
pub mod persona;
pub mod playlist;
pub mod prompts;
pub mod remaster;
pub mod speed;
pub mod stems;
pub mod types;
pub mod upload;

mod auth_retry;
mod client;
mod headers;
mod response;

#[cfg(test)]
mod endpoint_tests;

pub use client::SunoClient;

#[derive(Clone, Copy)]
pub struct PollingOptions {
    pub timeout: Duration,
    pub interval: Duration,
}

impl PollingOptions {
    pub(crate) fn validate(self) -> Result<(), crate::core::CliError> {
        crate::core::ensure_poll_interval(self.interval)?;
        crate::core::ensure_poll_timeout(self.timeout)
    }

    pub(crate) fn deadline(self) -> Result<tokio::time::Instant, crate::core::CliError> {
        self.validate()?;
        crate::core::deadline_after(self.timeout)
    }
}

#[cfg(test)]
mod polling_tests {
    use std::time::Duration;

    use super::PollingOptions;
    use crate::core::CliError;

    #[test]
    fn polling_options_reject_a_zero_interval() {
        let error = PollingOptions {
            timeout: Duration::from_secs(1),
            interval: Duration::ZERO,
        }
        .deadline()
        .expect_err("zero polling interval must be rejected");

        assert!(
            matches!(error, CliError::Config(message) if message.contains("poll interval") && message.contains("greater than 0"))
        );
    }
}
