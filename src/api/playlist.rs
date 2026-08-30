use serde_json::Value;

use super::SunoClient;
use super::mutation::MutationSpec;
use super::types::{
    CreatePlaylistRequest, PlaylistInfo, PlaylistListResponse, PlaylistReaction,
    PlaylistReorderRequest, PlaylistTrackMutationFailure, PlaylistTrackMutationReport,
    PlaylistTracksRequest, SetPlaylistCoverRequest, SetPlaylistMetadataRequest,
    SetPlaylistMetadataV2Request, SetPlaylistReactionRequest, SetPlaylistVisibilityRequest,
    TrashPlaylistRequest,
};
use crate::core::CliError;

impl SunoClient {
    /// List the authenticated user's playlists.
    /// GET /api/playlist/me?page={page}
    pub async fn list_playlists(&self, page: u32) -> Result<PlaylistListResponse, CliError> {
        self.with_auth_retry(|| async {
            self.read_json_with_transport_retry(
                self.get("/api/playlist/me").query(&[("page", page)]),
            )
            .await
        })
        .await
    }

    /// Fetch playlist details.
    /// GET /api/playlist/v2/{playlist_id}
    pub async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistInfo, CliError> {
        self.with_auth_retry(|| async {
            let raw = self
                .read_json_with_transport_retry(
                    self.get(&format!("/api/playlist/v2/{playlist_id}")),
                )
                .await?;
            let playlist = decode_playlist(raw)?;
            if playlist.id != playlist_id {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "playlist detail returned id `{}` while resolving `{playlist_id}`",
                        playlist.id
                    ),
                });
            }
            Ok(playlist)
        })
        .await
    }

    /// Create a playlist through Suno Web's name-only create route.
    pub async fn create_playlist(&self, name: &str) -> Result<PlaylistInfo, CliError> {
        let spec = playlist_mutation_spec("playlist_create", None);
        let body = self
            .mutation_json_once(
                self.post_without_redirect("/api/playlist/create/")
                    .json(&CreatePlaylistRequest {
                        name: name.to_string(),
                    }),
                &spec,
            )
            .await?;
        let playlist = decode_playlist(body).map_err(|error| {
            spec.ambiguous("response_schema", error.error_code(), error.to_string())
        })?;
        if playlist.id.trim().is_empty() || playlist.name != name {
            return Err(spec.ambiguous(
                "response_schema",
                "schema_drift",
                format!(
                    "playlist create returned id `{}` and name `{}`",
                    playlist.id, playlist.name
                ),
            ));
        }
        Ok(playlist)
    }

    /// Update playlist metadata through the current v2 route. The legacy
    /// endpoint remains an explicit compatibility path only for arbitrary
    /// external image URLs, which the v2 S3-cover contract cannot represent.
    pub async fn set_playlist_metadata(
        &self,
        playlist_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        image_url: Option<&str>,
    ) -> Result<(), CliError> {
        if image_url.is_none() {
            let req = SetPlaylistMetadataV2Request::new(name, description);
            let spec = playlist_mutation_spec("playlist_set_metadata", Some(playlist_id));
            self.send_mutation_once(
                self.patch_without_redirect(&format!("/api/playlist/v2/{playlist_id}"))
                    .json(&req),
                &spec,
            )
            .await?;
            return Ok(());
        }

        self.set_playlist_metadata_legacy(playlist_id, name, description, image_url)
            .await
    }

    async fn set_playlist_metadata_legacy(
        &self,
        playlist_id: &str,
        name: Option<&str>,
        description: Option<&str>,
        image_url: Option<&str>,
    ) -> Result<(), CliError> {
        let req = SetPlaylistMetadataRequest {
            playlist_id: playlist_id.to_string(),
            name: name.map(str::to_string),
            description: description.map(str::to_string),
            image_url: image_url.map(str::to_string),
        };

        let spec = playlist_mutation_spec("playlist_set_metadata_legacy", Some(playlist_id));
        let text = self
            .mutation_text_once(
                self.post_without_redirect("/api/playlist/set_metadata")
                    .json(&req),
                &spec,
            )
            .await?;
        if !text.trim().is_empty() {
            let body: Value = serde_json::from_str(&text).map_err(|error| {
                spec.ambiguous("response_body", "schema_drift", error.to_string())
            })?;
            reject_playlist_moderation_error(&body)?;
        }
        Ok(())
    }

    /// Set playlist cover to an image previously uploaded through Suno's image
    /// upload flow.
    /// PATCH /api/playlist/v2/{playlist_id}
    pub async fn set_playlist_uploaded_cover(
        &self,
        playlist_id: &str,
        upload_id: &str,
    ) -> Result<(), CliError> {
        let req = SetPlaylistCoverRequest::from_upload_id(upload_id);
        let spec = playlist_mutation_spec("playlist_set_cover", Some(playlist_id))
            .with_context("upload_id", Value::String(upload_id.to_string()));
        self.send_mutation_once(
            self.patch_without_redirect(&format!("/api/playlist/v2/{playlist_id}"))
                .json(&req),
            &spec,
        )
        .await?;
        Ok(())
    }

    /// Set or clear playlist like/dislike reaction.
    /// POST /api/playlist_reaction/{playlist_id}/update_reaction_type/
    pub async fn set_playlist_reaction(
        &self,
        playlist_id: &str,
        reaction: Option<PlaylistReaction>,
    ) -> Result<(), CliError> {
        let spec = playlist_mutation_spec("playlist_set_reaction", Some(playlist_id));
        self.send_mutation_once(
            self.post_without_redirect(&format!(
                "/api/playlist_reaction/{playlist_id}/update_reaction_type/"
            ))
            .json(&SetPlaylistReactionRequest::new(reaction)),
            &spec,
        )
        .await?;
        Ok(())
    }

    /// Add clips to a playlist.
    /// POST /api/playlist/v2/{playlist_id}/tracks/add
    pub async fn add_clips_to_playlist(
        &self,
        playlist_id: &str,
        clip_ids: &[String],
    ) -> Result<(), CliError> {
        self.update_playlist_tracks(playlist_id, "add", clip_ids)
            .await
    }

    /// Remove clips from a playlist.
    /// POST /api/playlist/v2/{playlist_id}/tracks/remove
    pub async fn remove_clips_from_playlist(
        &self,
        playlist_id: &str,
        clip_ids: &[String],
    ) -> Result<PlaylistTrackMutationReport, CliError> {
        let mut succeeded_clip_ids = Vec::new();
        let mut failed = Vec::new();
        let mut not_attempted_clip_ids = Vec::new();

        for (index, clip_id) in clip_ids.iter().enumerate() {
            match self
                .update_playlist_tracks(playlist_id, "remove", std::slice::from_ref(clip_id))
                .await
            {
                Ok(()) => succeeded_clip_ids.push(clip_id.clone()),
                Err(error) => {
                    if succeeded_clip_ids.is_empty() {
                        return Err(error);
                    }
                    failed.push(PlaylistTrackMutationFailure::from_error(clip_id, &error));
                    not_attempted_clip_ids.extend_from_slice(&clip_ids[index + 1..]);
                    break;
                }
            }
        }

        Ok(PlaylistTrackMutationReport::new(
            playlist_id,
            "remove",
            clip_ids,
            succeeded_clip_ids,
            failed,
            not_attempted_clip_ids,
        ))
    }

    /// Set playlist visibility.
    /// PATCH /api/playlist/v2/{playlist_id}
    pub async fn set_playlist_visibility(
        &self,
        playlist_id: &str,
        is_public: bool,
    ) -> Result<(), CliError> {
        let spec = playlist_mutation_spec("playlist_set_visibility", Some(playlist_id))
            .with_context("is_public", serde_json::json!(is_public));
        self.send_mutation_once(
            self.patch_without_redirect(&format!("/api/playlist/v2/{playlist_id}"))
                .json(&SetPlaylistVisibilityRequest::new(is_public)),
            &spec,
        )
        .await?;
        let readback = self
            .get_playlist(playlist_id)
            .await
            .map_err(|error| spec.ambiguous("readback", error.error_code(), error.to_string()))?;
        if readback.is_public != Some(is_public) {
            return Err(spec.ambiguous(
                "readback_state",
                "state_mismatch",
                format!(
                    "playlist visibility readback returned is_public={:?}",
                    readback.is_public
                ),
            ));
        }
        Ok(())
    }

    /// Save a playlist to the user's library.
    /// POST /api/playlist/v2/{playlist_id}/save
    pub async fn save_playlist(&self, playlist_id: &str) -> Result<(), CliError> {
        let spec = playlist_mutation_spec("playlist_save", Some(playlist_id));
        self.send_mutation_once(
            self.post_without_redirect(&format!("/api/playlist/v2/{playlist_id}/save")),
            &spec,
        )
        .await?;
        Ok(())
    }

    /// Remove a saved playlist from the user's library.
    /// DELETE /api/playlist/v2/{playlist_id}/save
    pub async fn unsave_playlist(&self, playlist_id: &str) -> Result<(), CliError> {
        let spec = playlist_mutation_spec("playlist_unsave", Some(playlist_id));
        self.send_mutation_once(
            self.delete_without_redirect(&format!("/api/playlist/v2/{playlist_id}/save")),
            &spec,
        )
        .await?;
        Ok(())
    }

    /// Move a playlist clip to a zero-based index.
    /// POST /api/playlist/v2/{playlist_id}/tracks/reorder-by-index
    pub async fn reorder_playlist_clip(
        &self,
        playlist_id: &str,
        clip_id: &str,
        index: u32,
    ) -> Result<(), CliError> {
        let spec = playlist_mutation_spec("playlist_reorder", Some(playlist_id))
            .with_context("clip_id", Value::String(clip_id.to_string()))
            .with_context("index", serde_json::json!(index));
        self.send_mutation_once(
            self.post_without_redirect(&format!(
                "/api/playlist/v2/{playlist_id}/tracks/reorder-by-index"
            ))
            .json(&PlaylistReorderRequest::single(clip_id, index)),
            &spec,
        )
        .await?;
        Ok(())
    }

    async fn update_playlist_tracks(
        &self,
        playlist_id: &str,
        action: &str,
        clip_ids: &[String],
    ) -> Result<(), CliError> {
        let operation = match action {
            "add" => "playlist_add_tracks",
            "remove" => "playlist_remove_tracks",
            _ => "playlist_update_tracks",
        };
        let spec = playlist_mutation_spec(operation, Some(playlist_id))
            .with_context("clip_ids", serde_json::json!(clip_ids));
        self.send_mutation_once(
            self.post_without_redirect(&format!("/api/playlist/v2/{playlist_id}/tracks/{action}"))
                .json(&PlaylistTracksRequest {
                    clip_ids: clip_ids.to_vec(),
                }),
            &spec,
        )
        .await?;
        Ok(())
    }

    /// Trash a playlist. The route supports undo, but the CLI exposes delete.
    /// POST /api/playlist/v2/{playlist_id}/trash
    pub async fn trash_playlist(&self, playlist_id: &str) -> Result<(), CliError> {
        self.set_playlist_trash_state(playlist_id, false).await
    }

    /// Restore a trashed playlist.
    /// POST /api/playlist/v2/{playlist_id}/trash
    pub async fn restore_playlist(&self, playlist_id: &str) -> Result<(), CliError> {
        self.set_playlist_trash_state(playlist_id, true).await
    }

    async fn set_playlist_trash_state(
        &self,
        playlist_id: &str,
        undo: bool,
    ) -> Result<(), CliError> {
        let operation = if undo {
            "playlist_restore"
        } else {
            "playlist_trash"
        };
        let spec = playlist_mutation_spec(operation, Some(playlist_id))
            .with_context("undo", serde_json::json!(undo));
        self.send_mutation_once(
            self.post_without_redirect(&format!("/api/playlist/v2/{playlist_id}/trash"))
                .json(&TrashPlaylistRequest { undo }),
            &spec,
        )
        .await?;
        let expected_trashed = !undo;
        let readback = self
            .get_playlist(playlist_id)
            .await
            .map_err(|error| spec.ambiguous("readback", error.error_code(), error.to_string()))?;
        if readback.is_trashed != Some(expected_trashed) {
            return Err(spec.ambiguous(
                "readback_state",
                "state_mismatch",
                format!(
                    "playlist trash readback returned is_trashed={:?}",
                    readback.is_trashed
                ),
            ));
        }
        Ok(())
    }
}

fn playlist_mutation_spec(operation: &'static str, playlist_id: Option<&str>) -> MutationSpec {
    let resource = playlist_id
        .map(|id| format!("playlist {id}"))
        .unwrap_or_else(|| "new playlist".to_string());
    let commands = playlist_id
        .map(|id| vec![format!("sunox playlist info {id} --json")])
        .unwrap_or_else(|| vec!["sunox playlist list --json".into()]);
    let mut spec = MutationSpec::new(operation, resource, commands);
    if let Some(playlist_id) = playlist_id {
        spec = spec.with_context("playlist_id", Value::String(playlist_id.to_string()));
    }
    spec
}

fn reject_playlist_moderation_error(body: &Value) -> Result<(), CliError> {
    if let Some(message) = body
        .get("moderation_error_message")
        .and_then(serde_json::Value::as_str)
    {
        return Err(CliError::Api {
            code: "moderation_error",
            message: message.to_string(),
        });
    }
    Ok(())
}

fn decode_playlist(body: Value) -> Result<PlaylistInfo, CliError> {
    let candidates = [
        body.get("playlist").cloned(),
        body.get("data").cloned(),
        Some(body.clone()),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Ok(playlist) = serde_json::from_value::<PlaylistInfo>(candidate) {
            return Ok(playlist);
        }
    }

    Err(CliError::Api {
        code: "schema_drift",
        message: format!("playlist response did not match known Suno schema: {body}"),
    })
}

pub(crate) fn upload_id_from_suno_image_url(url: &str) -> Option<String> {
    let url = url.trim().split(['?', '#']).next().unwrap_or_default();
    if !url.starts_with("https://cdn1.suno.ai/") && !url.starts_with("https://cdn2.suno.ai/") {
        return None;
    }
    let file = url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    let id = file
        .strip_prefix("image_")?
        .strip_suffix(".jpeg")
        .or_else(|| file.strip_prefix("image_")?.strip_suffix(".jpg"))?;
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::upload_id_from_suno_image_url;

    #[test]
    fn suno_image_url_extracts_upload_id() {
        assert_eq!(
            upload_id_from_suno_image_url("https://cdn2.suno.ai/image_upload-1.jpeg"),
            Some("upload-1".to_string())
        );
        assert_eq!(
            upload_id_from_suno_image_url("https://cdn1.suno.ai/image_upload-2.jpg?x=1"),
            Some("upload-2".to_string())
        );
        assert_eq!(
            upload_id_from_suno_image_url("https://example.com/image_upload-1.jpeg"),
            None
        );
    }
}
