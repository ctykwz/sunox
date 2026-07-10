use std::collections::HashSet;

use serde::Serialize;

use super::SunoClient;
use super::types::{ClipTrashRequest, FeedFilters};
use crate::core::CliError;

const PURGE_BATCH_SIZE: usize = 20;

#[derive(Serialize)]
struct PurgeClipsRequest<'a> {
    ids: &'a [String],
}

impl SunoClient {
    pub async fn delete_clips(&self, ids: &[String]) -> Result<(), CliError> {
        self.set_clip_trash(ids, true).await
    }

    pub async fn restore_clips(&self, ids: &[String]) -> Result<(), CliError> {
        self.set_clip_trash(ids, false).await
    }

    /// Permanently delete clips that are already in the Suno trash.
    /// POST /api/clips/delete/
    pub async fn purge_clips(&self, ids: &[String]) -> Result<(), CliError> {
        let mut purged = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(PURGE_BATCH_SIZE) {
            if let Err(error) = self.purge_clip_batch(chunk).await {
                if purged.is_empty() {
                    return Err(error);
                }
                let failed_end = purged.len() + chunk.len();
                return Err(CliError::PartialMutation {
                    message: "permanent clip deletion stopped before all clips were deleted".into(),
                    details: serde_json::json!({
                        "purged_clip_ids": purged,
                        "failed": {
                            "clip_ids": chunk,
                            "code": error.error_code(),
                            "message": error.to_string()
                        },
                        "not_attempted_clip_ids": &ids[failed_end..],
                    }),
                });
            }
            purged.extend_from_slice(chunk);
        }
        Ok(())
    }

    async fn purge_clip_batch(&self, ids: &[String]) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/clips/delete/")
                .json(&PurgeClipsRequest { ids })
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    pub async fn empty_clip_trash(&self) -> Result<Vec<String>, CliError> {
        let ids = self.trashed_clip_ids().await?;
        self.purge_clips(&ids).await?;
        Ok(ids)
    }

    async fn trashed_clip_ids(&self) -> Result<Vec<String>, CliError> {
        let mut cursor = None;
        let mut seen_cursors = HashSet::new();
        let mut seen_ids = HashSet::new();
        let mut ids = Vec::new();

        loop {
            let feed = self
                .feed(cursor.take(), Some(20), FeedFilters::trashed())
                .await?;
            ids.extend(
                feed.clips
                    .into_iter()
                    .map(|clip| clip.id)
                    .filter(|id| seen_ids.insert(id.clone())),
            );

            if !feed.has_more {
                return Ok(ids);
            }
            let next_cursor = feed.next_cursor.ok_or_else(|| CliError::Api {
                code: "schema_drift",
                message: "Suno reported more trashed clips without a pagination cursor".into(),
            })?;
            if !seen_cursors.insert(next_cursor.clone()) {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: "Suno repeated a trash pagination cursor".into(),
                });
            }
            cursor = Some(next_cursor);
        }
    }

    async fn set_clip_trash(&self, ids: &[String], trash: bool) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/gen/trash")
                .json(&ClipTrashRequest {
                    trash,
                    clip_ids: ids.to_vec(),
                })
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }
}
