use serde_json::Value;

use super::SunoClient;
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
            let resp = self
                .get("/api/playlist/me")
                .query(&[("page", page)])
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Fetch playlist details.
    /// GET /api/playlist/v2/{playlist_id}
    pub async fn get_playlist(&self, playlist_id: &str) -> Result<PlaylistInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/playlist/v2/{playlist_id}"))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            decode_playlist(resp.json().await?)
        })
        .await
    }

    /// Create a playlist through Suno Web's name-only create route.
    pub async fn create_playlist(&self, name: &str) -> Result<PlaylistInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/playlist/create/")
                .json(&CreatePlaylistRequest {
                    name: name.to_string(),
                })
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            decode_playlist(resp.json().await?)
        })
        .await
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
            return self
                .with_auth_retry(|| async {
                    let resp = self
                        .patch(&format!("/api/playlist/v2/{playlist_id}"))
                        .json(&req)
                        .send()
                        .await?;
                    self.check_response(resp).await?;
                    Ok(())
                })
                .await;
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

        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/playlist/set_metadata")
                .json(&req)
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let text = resp.text().await.unwrap_or_default();
            if !text.trim().is_empty() {
                let body: Value = serde_json::from_str(&text)?;
                reject_playlist_moderation_error(&body)?;
            }
            Ok(())
        })
        .await
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
        self.with_auth_retry(|| async {
            let resp = self
                .patch(&format!("/api/playlist/v2/{playlist_id}"))
                .json(&req)
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    /// Set or clear playlist like/dislike reaction.
    /// POST /api/playlist_reaction/{playlist_id}/update_reaction_type/
    pub async fn set_playlist_reaction(
        &self,
        playlist_id: &str,
        reaction: Option<PlaylistReaction>,
    ) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!(
                    "/api/playlist_reaction/{playlist_id}/update_reaction_type/"
                ))
                .json(&SetPlaylistReactionRequest::new(reaction))
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
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
        self.with_auth_retry(|| async {
            let resp = self
                .patch(&format!("/api/playlist/v2/{playlist_id}"))
                .json(&SetPlaylistVisibilityRequest::new(is_public))
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    /// Save a playlist to the user's library.
    /// POST /api/playlist/v2/{playlist_id}/save
    pub async fn save_playlist(&self, playlist_id: &str) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/playlist/v2/{playlist_id}/save"))
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    /// Remove a saved playlist from the user's library.
    /// DELETE /api/playlist/v2/{playlist_id}/save
    pub async fn unsave_playlist(&self, playlist_id: &str) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .delete(&format!("/api/playlist/v2/{playlist_id}/save"))
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    /// Move a playlist clip to a zero-based index.
    /// POST /api/playlist/v2/{playlist_id}/tracks/reorder-by-index
    pub async fn reorder_playlist_clip(
        &self,
        playlist_id: &str,
        clip_id: &str,
        index: u32,
    ) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!(
                    "/api/playlist/v2/{playlist_id}/tracks/reorder-by-index"
                ))
                .json(&PlaylistReorderRequest::single(clip_id, index))
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }

    async fn update_playlist_tracks(
        &self,
        playlist_id: &str,
        action: &str,
        clip_ids: &[String],
    ) -> Result<(), CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/playlist/v2/{playlist_id}/tracks/{action}"))
                .json(&PlaylistTracksRequest {
                    clip_ids: clip_ids.to_vec(),
                })
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
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
        self.with_auth_retry(|| async {
            let resp = self
                .post(&format!("/api/playlist/v2/{playlist_id}/trash"))
                .json(&TrashPlaylistRequest { undo })
                .send()
                .await?;
            self.check_response(resp).await?;
            Ok(())
        })
        .await
    }
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
