//! Per-command recovery evidence. Nothing is written for read-only commands.
//! Only route names and allowlisted identifiers are persisted, never request payloads or auth.

use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, TryLockError};

use serde::Serialize;
use serde_json::{Value, json};

use super::CliError;

mod identities;
use identities::Identifiers;

tokio::task_local! {
    static CURRENT: OperationRecovery;
}

#[derive(Clone)]
pub(crate) struct OperationRecovery(
    Arc<Mutex<RecoveryState>>,
    Arc<AtomicBool>,
    Arc<RecoveryIdentity>,
);

struct RecoveryIdentity {
    operation_id: String,
    checkpoint_path: Option<PathBuf>,
}

struct RecoveryState {
    directory: Option<PathBuf>,
    checkpoint: Checkpoint,
    retain_on_success: bool,
}

#[derive(Serialize)]
struct Checkpoint {
    version: u8,
    operation_id: String,
    state: &'static str,
    writes: Vec<WriteRecord>,
}

#[derive(Serialize)]
struct WriteRecord {
    method: String,
    path: String,
    account_key: Option<String>,
    state: &'static str,
    identifiers: Identifiers,
}

impl OperationRecovery {
    pub(crate) fn new() -> Self {
        Self::with_directory(super::project_config_dir().map(|dir| dir.join("operations")))
    }

    fn with_directory(directory: Option<PathBuf>) -> Self {
        let operation_id = uuid::Uuid::new_v4().to_string();
        let checkpoint_path = directory
            .as_ref()
            .map(|directory| directory.join(format!("{operation_id}.json")));
        Self(
            Arc::new(Mutex::new(RecoveryState {
                directory,
                retain_on_success: false,
                checkpoint: Checkpoint {
                    version: 1,
                    operation_id: operation_id.clone(),
                    state: "running",
                    writes: Vec::new(),
                },
            })),
            Arc::new(AtomicBool::new(false)),
            Arc::new(RecoveryIdentity {
                operation_id,
                checkpoint_path,
            }),
        )
    }

    #[cfg(test)]
    fn lock_state_for_test(&self) -> std::sync::MutexGuard<'_, RecoveryState> {
        self.0.lock().expect("operation recovery mutex")
    }

    pub(crate) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(crate) fn cancel(&self) {
        self.1.store(true, Ordering::Release);
    }

    /// Preserve recovery evidence on failure; cancellation is never described as remote rollback.
    pub(crate) fn error_details(&self, error: &CliError) -> Option<Value> {
        let interrupted = matches!(error, CliError::Interrupted);
        let mut state = if interrupted {
            match self.0.try_lock() {
                Ok(state) => state,
                Err(TryLockError::Poisoned(error)) => error.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    let mut details = error.details().cloned().unwrap_or_else(|| json!({}));
                    if !details.is_object() {
                        details = json!({"cause_details": details});
                    }
                    details["operation_recovery"] = json!({
                        "operation_id": self.2.operation_id,
                        "checkpoint_path": self.2.checkpoint_path,
                        "state": "interrupted",
                        "remote_effects_possible": true,
                        "resumable": false,
                        "writes": [],
                        "reason": "Recovery persistence was still in progress when the command was interrupted. Inspect the checkpoint and remote resources before submitting again.",
                        "inspection_commands": ["sunox clip list --json", "sunox credits --json"],
                    });
                    return Some(details);
                }
            }
        } else {
            self.0.lock().expect("operation recovery mutex")
        };
        if state.checkpoint.writes.is_empty() {
            return error.details().cloned();
        }
        state.checkpoint.state = if self.1.load(Ordering::Acquire) {
            "interrupted"
        } else {
            "failed"
        };
        // Every write transition was already persisted before this point. Avoid synchronous
        // filesystem work on SIGINT so the bounded shutdown path cannot hang on a slow volume.
        let persist_error = if interrupted {
            None
        } else {
            state.persist().err().map(|error| error.to_string())
        };
        let mut recovery = state.details();
        if let Some(error) = persist_error {
            recovery["checkpoint_error"] = Value::String(error);
        }
        let mut details = error.details().cloned().unwrap_or_else(|| json!({}));
        if !details.is_object() {
            details = json!({"cause_details": details});
        }
        details["operation_recovery"] = recovery;
        Some(details)
    }

    /// Unresolved optional writes outlive successful commands; reconciled writes do not.
    pub(crate) fn finish(&self) -> Result<(), CliError> {
        let mut state = self.0.lock().expect("operation recovery mutex");
        if state.checkpoint.writes.is_empty() {
            return Ok(());
        }
        if state.retain_on_success {
            state.checkpoint.state = "completed_with_warnings";
            return state.persist();
        }
        state.checkpoint.state = "completed";
        state.persist()?;
        std::fs::remove_file(state.path()?)?;
        Ok(())
    }
}

impl RecoveryState {
    fn path(&self) -> Result<PathBuf, CliError> {
        self.directory
            .as_ref()
            .map(|directory| directory.join(format!("{}.json", self.checkpoint.operation_id)))
            .ok_or_else(|| {
                CliError::Config("cannot resolve the operation recovery directory".into())
            })
    }

    fn persist(&self) -> Result<(), CliError> {
        let path = self.path()?;
        let directory = path.parent().expect("checkpoint has a parent");
        std::fs::create_dir_all(directory)?;
        let metadata = std::fs::symlink_metadata(directory)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CliError::Config(
                "operation recovery directory must be a regular directory".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(&mut temporary, &self.checkpoint)?;
        temporary.flush()?;
        temporary.as_file().sync_all()?;
        temporary
            .persist(&path)
            .map_err(|error| CliError::Io(error.error))?;
        #[cfg(unix)]
        std::fs::File::open(directory)?.sync_all()?;
        Ok(())
    }

    fn details(&self) -> Value {
        let mut inspection = vec![
            "sunox clip list --json".to_string(),
            "sunox credits --json".to_string(),
        ];
        for write in &self.checkpoint.writes {
            for (key, command) in [
                ("upload_ids", "sunox clip upload-status"),
                ("clip_ids", "sunox clip info"),
                ("playlist_ids", "sunox playlist info"),
                ("persona_ids", "sunox persona info"),
                ("lyrics_project_ids", "sunox lyrics projects info"),
                ("processed_ids", "sunox voice processed-status"),
                ("verification_ids", "sunox voice verification-status"),
                ("mashup_ids", "sunox lyrics mashup-status"),
            ] {
                for id in write.identifiers.get(key).into_iter().flatten() {
                    let command = format!("{command} {id} --json");
                    if !inspection.contains(&command) {
                        inspection.push(command);
                    }
                }
            }
            let listing = if write.path.starts_with("/api/playlist") {
                Some("sunox playlist list --json")
            } else if write.path.starts_with("/api/persona/") {
                Some("sunox persona list --json")
            } else if write.path.starts_with("/api/lyrics-projects") {
                Some("sunox lyrics projects list --json")
            } else if write.path.starts_with("/api/custom-model/") {
                Some("sunox models custom pending --json")
            } else {
                None
            };
            if let Some(command) = listing
                && !inspection.iter().any(|existing| existing == command)
            {
                inspection.push(command.to_string());
            }
            let media = match write.path.as_str() {
                "/api/video_gen/image/generate" => Some("image"),
                "/api/video_gen/video/generate" => Some("video"),
                _ => None,
            };
            if let Some(media) = media {
                for id in write.identifiers.get("batch_ids").into_iter().flatten() {
                    let command =
                        format!("sunox clip cover-art status {id} --media {media} --json");
                    if !inspection.contains(&command) {
                        inspection.push(command);
                    }
                }
            }
        }
        json!({
            "operation_id": self.checkpoint.operation_id,
            "checkpoint_path": self.path().ok(),
            "state": self.checkpoint.state,
            "remote_effects_possible": true,
            "resumable": false,
            "writes": self.checkpoint.writes,
            "reason": "A write started before the command stopped. Stopping the CLI does not cancel or roll back Suno work; inspect these resources before submitting again.",
            "inspection_commands": inspection,
        })
    }
}

pub(crate) fn preserve_warning_details(error: &CliError) -> Option<Value> {
    if !matches!(
        error,
        CliError::AmbiguousMutation { .. } | CliError::PartialMutation { .. }
    ) {
        return error.details().cloned();
    }
    CURRENT
        .try_with(|current| {
            current
                .0
                .lock()
                .expect("operation recovery mutex")
                .retain_on_success = true;
            current.error_details(error)
        })
        .unwrap_or_else(|_| error.details().cloned())
}

pub(crate) fn is_active() -> bool {
    CURRENT.try_with(|_| ()).is_ok()
}

/// Called immediately before every account write. Persistence failure prevents sending it.
pub(crate) fn record_request(
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    account_key: Option<String>,
    context: &[(&'static str, Value)],
) -> Result<(), CliError> {
    CURRENT
        .try_with(|current| {
            let mut state = current.0.lock().expect("operation recovery mutex");
            if current.1.load(Ordering::Acquire) {
                return Err(CliError::Interrupted);
            }
            let mut write = WriteRecord {
                method: method.into(),
                path: path.into(),
                account_key,
                state: "possibly_sent",
                identifiers: Identifiers::new(),
            };
            let body = body.and_then(|body| serde_json::from_slice::<Value>(body).ok());
            identities::request(path, body.as_ref(), context, &mut write.identifiers);
            state.checkpoint.writes.push(write);
            if let Err(error) = state.persist() {
                // The caller has not sent this request. Retain any earlier writes, but do not
                // describe this pre-send local failure as an additional remote mutation.
                state.checkpoint.writes.pop();
                return Err(CliError::Config(format!(
                    "cannot save operation recovery before sending the write: {error}"
                )));
            }
            Ok(())
        })
        .unwrap_or(Ok(()))
}

/// A parsed successful response is checkpointed before its caller starts the next stage.
pub(crate) fn record_response(path: &str, response: &Value) -> Result<(), CliError> {
    CURRENT
        .try_with(|current| {
            let mut state = current.0.lock().expect("operation recovery mutex");
            let Some(write) = state
                .checkpoint
                .writes
                .iter_mut()
                .rev()
                .find(|write| write.path == path)
            else {
                return Ok(());
            };
            write.state = "response_received";
            identities::response(path, response, &mut write.identifiers);
            state.persist()
        })
        .unwrap_or(Ok(()))
}

/// A successful HTTP response makes the outcome known even when the caller does not
/// consume a response body. Later parsing can still fail, but the write must no longer
/// be described as having lost its response.
pub(crate) fn record_acknowledgement(path: &str) -> Result<(), CliError> {
    CURRENT
        .try_with(|current| {
            let mut state = current.0.lock().expect("operation recovery mutex");
            let Some(write) = state
                .checkpoint
                .writes
                .iter_mut()
                .rev()
                .find(|write| write.path == path && write.state == "possibly_sent")
            else {
                return Ok(());
            };
            write.state = "response_received";
            state.persist()
        })
        .unwrap_or(Ok(()))
}

/// A received 4xx response proves that the server rejected this request. Remove the
/// conservative pre-send record without disturbing earlier writes in the command.
pub(crate) fn record_rejection(path: &str) -> Result<(), CliError> {
    CURRENT
        .try_with(|current| {
            let mut state = current.0.lock().expect("operation recovery mutex");
            let Some(index) = state
                .checkpoint
                .writes
                .iter()
                .rposition(|write| write.path == path && write.state == "possibly_sent")
            else {
                return Ok(());
            };
            let rejected = state.checkpoint.writes.remove(index);
            let result = if state.checkpoint.writes.is_empty() {
                match std::fs::remove_file(state.path()?) {
                    Ok(()) => Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(error) => Err(CliError::Io(error)),
                }
            } else {
                state.persist()
            };
            if result.is_err() {
                state.checkpoint.writes.insert(index, rejected);
            }
            result
        })
        .unwrap_or(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_preserves_identity_before_send_but_excludes_secrets() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery.scope(async {
            record_request("POST", "/api/generate/v2-web/", Some(br#"{"transaction_uuid":"transaction-1","prompt":"PRIVATE LYRICS","token":"SECRET CAPTCHA","cookie":"SECRET COOKIE"}"#), Some("account-hash".into()), &[]).expect("record request");
            let state = recovery.0.lock().expect("state");
            let stored = std::fs::read_to_string(state.path().expect("path")).expect("checkpoint before response");
            assert!(stored.contains("transaction-1"));
            for forbidden in ["PRIVATE LYRICS", "SECRET CAPTCHA", "SECRET COOKIE"] {
                assert!(!stored.contains(forbidden));
            }
        }).await;
        recovery.cancel();
        let details = recovery
            .error_details(&CliError::Interrupted)
            .expect("interruption details");
        assert_eq!(details["operation_recovery"]["state"], "interrupted");
        assert_eq!(
            details["operation_recovery"]["remote_effects_possible"],
            true
        );
    }

    #[tokio::test]
    async fn upload_id_is_saved_before_the_next_stage_and_success_removes_checkpoint() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery.scope(async {
            record_request("POST", "/api/uploads/audio/", None, None, &[]).expect("request");
            record_response("/api/uploads/audio/", &json!({"id":"upload-1", "url":"https://secret-presigned-url", "fields":{"signature":"SECRET"}})).expect("response");
        }).await;
        let details = recovery
            .error_details(&CliError::Interrupted)
            .expect("details");
        assert!(
            details
                .to_string()
                .contains("sunox clip upload-status upload-1 --json")
        );
        assert!(!details.to_string().contains("secret-presigned"));
        let path = recovery.0.lock().expect("state").path().expect("path");
        assert!(path.exists());
        recovery.finish().expect("finish");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn cancelled_before_a_write_does_not_start_or_create_a_checkpoint() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery.cancel();
        recovery
            .scope(async {
                assert!(matches!(
                    record_request("POST", "/api/generate/v2-web/", None, None, &[]),
                    Err(CliError::Interrupted)
                ));
            })
            .await;
        assert!(recovery.error_details(&CliError::Interrupted).is_none());
        assert!(!directory.path().join("operations").exists());
    }

    #[test]
    fn interrupted_error_details_never_wait_for_a_busy_checkpoint_lock() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery.cancel();
        let _state = recovery.lock_state_for_test();

        let details = recovery
            .error_details(&CliError::Interrupted)
            .expect("conservative recovery details");

        assert_eq!(details["operation_recovery"]["state"], "interrupted");
        assert_eq!(
            details["operation_recovery"]["remote_effects_possible"],
            true
        );
        assert!(details["operation_recovery"]["checkpoint_path"].is_string());
    }

    #[tokio::test]
    async fn definite_rejection_removes_only_the_rejected_write() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery
            .scope(async {
                record_request("POST", "/api/first", None, None, &[]).expect("first");
                record_response("/api/first", &json!({"id": "known-first"}))
                    .expect("first response");
                record_request("POST", "/api/rejected", None, None, &[]).expect("rejected request");
                record_rejection("/api/rejected").expect("discard rejection");
            })
            .await;

        let details = recovery
            .error_details(&CliError::Interrupted)
            .expect("earlier write remains recoverable");
        let writes = details["operation_recovery"]["writes"]
            .as_array()
            .expect("writes");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0]["path"], "/api/first");
    }

    #[tokio::test]
    async fn successful_acknowledgement_marks_the_write_as_received() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery
            .scope(async {
                record_request("PATCH", "/api/acknowledged", None, None, &[]).expect("request");
                record_acknowledgement("/api/acknowledged").expect("acknowledgement");
            })
            .await;

        let details = recovery
            .error_details(&CliError::Interrupted)
            .expect("acknowledged write remains recoverable");
        assert_eq!(
            details["operation_recovery"]["writes"][0]["state"],
            "response_received"
        );
    }

    #[tokio::test]
    async fn sole_definite_rejection_removes_the_checkpoint() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let recovery = OperationRecovery::with_directory(Some(directory.path().join("operations")));
        recovery
            .scope(async {
                record_request("POST", "/api/rejected", None, None, &[]).expect("rejected request");
                record_rejection("/api/rejected").expect("discard rejection");
            })
            .await;

        assert!(recovery.error_details(&CliError::Interrupted).is_none());
        assert_eq!(
            std::fs::read_dir(directory.path().join("operations"))
                .expect("operations directory")
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn unresolved_warning_survives_later_successful_writes_and_finish() {
        for received in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let recovery = OperationRecovery::with_directory(Some(directory.path().to_path_buf()));
            let details = recovery
                .scope(async {
                    record_request(
                        "POST",
                        "/api/gen/clip-first/aligned_lyrics/v3",
                        None,
                        None,
                        &[],
                    )
                    .unwrap();
                    if received {
                        record_acknowledgement("/api/gen/clip-first/aligned_lyrics/v3").unwrap();
                    }
                    let details = preserve_warning_details(&CliError::AmbiguousMutation {
                        message: "response lost".into(),
                        details: json!({"operation_id":"first-write"}),
                    })
                    .unwrap();
                    record_request(
                        "POST",
                        "/api/gen/clip-next/aligned_lyrics/v3",
                        None,
                        None,
                        &[],
                    )
                    .unwrap();
                    record_response(
                        "/api/gen/clip-next/aligned_lyrics/v3",
                        &json!({"alignment":[]}),
                    )
                    .unwrap();
                    details
                })
                .await;
            recovery.finish().unwrap();
            let path = details["operation_recovery"]["checkpoint_path"]
                .as_str()
                .unwrap();
            let checkpoint: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(checkpoint["state"], "completed_with_warnings");
            assert_eq!(checkpoint["writes"].as_array().unwrap().len(), 2);
            assert_eq!(details["operation_id"], "first-write");
        }
    }

    #[tokio::test]
    async fn read_only_warning_does_not_retain_successful_mutation_history() {
        let directory = tempfile::tempdir().unwrap();
        let recovery = OperationRecovery::with_directory(Some(directory.path().to_path_buf()));
        recovery
            .scope(async {
                record_request("POST", "/api/download/authorize", None, None, &[]).unwrap();
                record_response("/api/download/authorize", &json!({"ok":true})).unwrap();
                assert!(preserve_warning_details(&CliError::RateLimited).is_none());
            })
            .await;
        recovery.finish().unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn unwritable_checkpoint_prevents_the_write() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let blocked = directory.path().join("not-a-directory");
        std::fs::write(&blocked, "existing file").expect("block directory");
        let recovery = OperationRecovery::with_directory(Some(blocked));
        recovery
            .scope(async {
                let error = record_request("POST", "/api/generate/v2-web/", None, None, &[])
                    .expect_err("must fail closed");
                assert!(error.to_string().contains("before sending"));
            })
            .await;
    }
}
