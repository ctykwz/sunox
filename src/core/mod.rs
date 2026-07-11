//! Shared application primitives used across domains.

mod config;
mod error;
mod paths;
mod polling;
mod validation;

pub use config::{AppConfig, ensure_poll_timeout_secs};
pub use error::CliError;
pub(crate) use paths::{project_config_dir, user_home_dir};
pub(crate) use polling::{
    deadline_after, ensure_poll_interval, ensure_poll_timeout, run_before_deadline,
    sleep_before_deadline,
};
pub use validation::{
    ensure_clip_ids, ensure_destructive_confirmed, ensure_non_negative_finite, ensure_percentage,
};
