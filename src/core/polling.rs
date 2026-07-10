use std::future::Future;

use tokio::time::{Instant, timeout_at};

use super::CliError;

pub(crate) fn ensure_poll_interval(interval: std::time::Duration) -> Result<(), CliError> {
    if interval.is_zero() {
        return Err(CliError::Config(
            "poll interval must be greater than 0 seconds".into(),
        ));
    }
    Ok(())
}

pub(crate) fn ensure_poll_timeout(timeout: std::time::Duration) -> Result<(), CliError> {
    checked_deadline(timeout).map(|_| ())
}

pub(crate) fn deadline_after(timeout: std::time::Duration) -> Result<Instant, CliError> {
    checked_deadline(timeout)
}

fn checked_deadline(timeout: std::time::Duration) -> Result<Instant, CliError> {
    if timeout.is_zero() {
        return Err(CliError::Config(
            "poll timeout must be greater than 0 seconds".into(),
        ));
    }
    Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| CliError::Config("poll timeout is too large".into()))
}

pub(crate) async fn run_before_deadline<T, F>(
    deadline: Instant,
    future: F,
    timeout_error: CliError,
) -> Result<T, CliError>
where
    F: Future<Output = Result<T, CliError>>,
{
    match timeout_at(deadline, future).await {
        Ok(result) => result,
        Err(_) => Err(timeout_error),
    }
}

pub(crate) async fn sleep_before_deadline(
    deadline: Instant,
    interval: std::time::Duration,
) -> bool {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return false;
    }
    tokio::time::sleep(interval.min(remaining)).await;
    Instant::now() < deadline
}
