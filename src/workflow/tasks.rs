use std::time::Duration;

use tokio::time::Instant;

use crate::api::SunoClient;
use crate::api::types::Clip;
use crate::core::{CliError, deadline_after, run_before_deadline, sleep_before_deadline};

pub(crate) const MAX_POLL_BACKOFF: Duration = Duration::from_secs(15);

pub fn is_terminal_status(status: &str) -> bool {
    status == "complete"
}

pub fn require_found_clips(ids: &[String], clips: Vec<Clip>) -> Result<Vec<Clip>, CliError> {
    let missing = ids
        .iter()
        .filter(|id| !clips.iter().any(|clip| clip.id == **id))
        .cloned()
        .collect::<Vec<_>>();

    if !missing.is_empty() {
        return Err(CliError::NotFound(format!(
            "clip(s): {}",
            missing.join(", ")
        )));
    }

    Ok(clips)
}

pub async fn wait_for_clips(
    client: &SunoClient,
    ids: &[String],
    timeout_secs: u64,
    poll_interval_secs: u64,
) -> Result<Vec<Clip>, CliError> {
    let timeout = Duration::from_secs(timeout_secs);
    let deadline = deadline_after(timeout)?;
    let mut delay = Duration::from_secs(poll_interval_secs.max(1));
    let mut last_missing_ids = Vec::new();

    loop {
        let clips = run_before_deadline(
            deadline,
            client.get_clips(ids),
            wait_timeout_error(ids, &last_missing_ids, timeout_secs),
        )
        .await?;
        let failed_ids = clips
            .iter()
            .filter(|clip| matches!(clip.status.as_str(), "error" | "failed"))
            .map(|clip| clip.id.as_str())
            .collect::<Vec<_>>();
        if !failed_ids.is_empty() {
            return Err(CliError::GenerationFailed(format!(
                "generation failed for {}",
                failed_ids.join(", ")
            )));
        }
        let missing_ids = ids
            .iter()
            .filter(|id| !clips.iter().any(|clip| clip.id == **id))
            .cloned()
            .collect::<Vec<_>>();
        if missing_ids.is_empty() && clips.iter().all(|clip| is_terminal_status(&clip.status)) {
            return Ok(clips);
        }
        if Instant::now() >= deadline {
            return Err(wait_timeout_error(ids, &missing_ids, timeout_secs));
        }
        last_missing_ids = missing_ids;
        if !sleep_before_deadline(deadline, delay).await {
            return Err(wait_timeout_error(ids, &last_missing_ids, timeout_secs));
        }
        delay = (delay * 2).min(MAX_POLL_BACKOFF);
    }
}

fn wait_timeout_error(ids: &[String], missing_ids: &[String], timeout_secs: u64) -> CliError {
    if !missing_ids.is_empty() {
        return CliError::NotFound(format!(
            "clip(s) after waiting {timeout_secs}s: {}",
            missing_ids.join(", ")
        ));
    }
    CliError::GenerationFailed(format!(
        "generation timed out after {timeout_secs}s for {}",
        ids.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(id: &str, status: &str) -> Clip {
        Clip {
            id: id.into(),
            title: format!("Clip {id}"),
            status: status.into(),
            model_name: "chirp-fenix".into(),
            audio_url: None,
            video_url: None,
            image_url: None,
            created_at: "2026-06-30T00:00:00Z".into(),
            play_count: 0,
            upvote_count: 0,
            metadata: Default::default(),
        }
    }

    #[test]
    fn only_complete_is_a_successful_terminal_state() {
        assert!(is_terminal_status("complete"));
        assert!(!is_terminal_status("error"));
    }

    #[test]
    fn streaming_and_submitted_are_not_terminal_states() {
        assert!(!is_terminal_status("streaming"));
        assert!(!is_terminal_status("submitted"));
    }

    #[test]
    fn found_clips_rejects_empty_response_for_requested_ids() {
        let ids = vec!["clip-missing".to_string()];

        let err = require_found_clips(&ids, Vec::new()).expect_err("missing clip should fail");

        assert!(matches!(err, CliError::NotFound(message) if message.contains("clip-missing")));
    }

    #[test]
    fn found_clips_rejects_partial_response_for_requested_ids() {
        let ids = vec!["clip-a".to_string(), "clip-b".to_string()];

        let err = require_found_clips(&ids, vec![clip("clip-a", "complete")])
            .expect_err("partial clip response should fail");

        assert!(
            matches!(err, CliError::NotFound(message) if message.contains("clip-b") && !message.contains("clip-a"))
        );
    }

    #[test]
    fn found_clips_returns_complete_response() {
        let ids = vec!["clip-a".to_string(), "clip-b".to_string()];
        let clips = vec![clip("clip-a", "complete"), clip("clip-b", "submitted")];

        let clips = require_found_clips(&ids, clips).expect("all requested clips found");

        assert_eq!(clips.len(), 2);
    }
}
