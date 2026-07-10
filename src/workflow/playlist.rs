use crate::api::SunoClient;
use crate::api::types::PlaylistInfo;
use crate::core::CliError;

#[derive(Clone, Copy)]
pub struct CoverReference<'a> {
    upload_id: &'a str,
    image_url: &'a str,
    uploaded_here: bool,
}

impl<'a> CoverReference<'a> {
    pub fn existing(upload_id: &'a str, image_url: &'a str) -> Self {
        Self {
            upload_id,
            image_url,
            uploaded_here: false,
        }
    }

    pub fn uploaded(upload_id: &'a str, image_url: &'a str) -> Self {
        Self {
            upload_id,
            image_url,
            uploaded_here: true,
        }
    }
}

pub struct CreatePlaylistInput<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub external_image_url: Option<&'a str>,
    pub cover: Option<CoverReference<'a>>,
}

pub struct SetPlaylistInput<'a> {
    pub playlist_id: &'a str,
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub external_image_url: Option<&'a str>,
    pub cover: Option<CoverReference<'a>>,
}

pub async fn create(
    client: &SunoClient,
    input: CreatePlaylistInput<'_>,
) -> Result<PlaylistInfo, CliError> {
    let mut completed_steps = Vec::new();
    if input.cover.is_some_and(|cover| cover.uploaded_here) {
        completed_steps.push("cover_uploaded");
    }
    let created = client.create_playlist(input.name).await.map_err(|error| {
        stage_error(
            "playlist_create",
            None,
            input.cover,
            &completed_steps,
            "playlist_create",
            error,
        )
    })?;
    let playlist_id = created.id.clone();
    completed_steps.push("playlist_created");

    if input.description.is_some() || input.external_image_url.is_some() {
        client
            .set_playlist_metadata(
                &playlist_id,
                None,
                input.description,
                input.external_image_url,
            )
            .await
            .map_err(|error| {
                stage_error(
                    "playlist_create",
                    Some(&playlist_id),
                    input.cover,
                    &completed_steps,
                    "metadata_update",
                    error,
                )
            })?;
        completed_steps.push("metadata_updated");
    }

    if let Some(cover) = input.cover {
        client
            .set_playlist_uploaded_cover(&playlist_id, cover.upload_id)
            .await
            .map_err(|error| {
                stage_error(
                    "playlist_create",
                    Some(&playlist_id),
                    input.cover,
                    &completed_steps,
                    "cover_update",
                    error,
                )
            })?;
        completed_steps.push("cover_updated");
    }

    if completed_steps.len() == 1 {
        return Ok(created);
    }
    client.get_playlist(&playlist_id).await.map_err(|error| {
        stage_error(
            "playlist_create",
            Some(&playlist_id),
            input.cover,
            &completed_steps,
            "readback",
            error,
        )
    })
}

pub async fn set(
    client: &SunoClient,
    input: SetPlaylistInput<'_>,
) -> Result<PlaylistInfo, CliError> {
    let mut completed_steps = Vec::new();
    if input.cover.is_some_and(|cover| cover.uploaded_here) {
        completed_steps.push("cover_uploaded");
    }

    if input.name.is_some() || input.description.is_some() || input.external_image_url.is_some() {
        client
            .set_playlist_metadata(
                input.playlist_id,
                input.name,
                input.description,
                input.external_image_url,
            )
            .await
            .map_err(|error| {
                stage_error(
                    "playlist_set",
                    Some(input.playlist_id),
                    input.cover,
                    &completed_steps,
                    "metadata_update",
                    error,
                )
            })?;
        completed_steps.push("metadata_updated");
    }

    if let Some(cover) = input.cover {
        client
            .set_playlist_uploaded_cover(input.playlist_id, cover.upload_id)
            .await
            .map_err(|error| {
                stage_error(
                    "playlist_set",
                    Some(input.playlist_id),
                    input.cover,
                    &completed_steps,
                    "cover_update",
                    error,
                )
            })?;
        completed_steps.push("cover_updated");
    }

    client
        .get_playlist(input.playlist_id)
        .await
        .map_err(|error| {
            stage_error(
                "playlist_set",
                Some(input.playlist_id),
                input.cover,
                &completed_steps,
                "readback",
                error,
            )
        })
}

fn stage_error(
    operation: &str,
    playlist_id: Option<&str>,
    cover: Option<CoverReference<'_>>,
    completed_steps: &[&str],
    failed_step: &str,
    error: CliError,
) -> CliError {
    if completed_steps.is_empty() {
        return error;
    }
    let resource = playlist_id.unwrap_or("not-created");
    let mut details = serde_json::json!({
        "operation": operation,
        "completed_steps": completed_steps,
        "failed": {
            "step": failed_step,
            "code": error.error_code(),
            "message": error.to_string()
        }
    });
    if let Some(playlist_id) = playlist_id {
        details["playlist_id"] = serde_json::Value::String(playlist_id.to_string());
    }
    if let Some(cover) = cover {
        details["cover"] = serde_json::json!({
            "upload_id": cover.upload_id,
            "image_url": cover.image_url,
            "uploaded_here": cover.uploaded_here,
        });
    }
    details["recovery"] = recovery_details(failed_step, playlist_id, cover);
    CliError::PartialMutation {
        message: format!(
            "{operation} for {resource} stopped at {failed_step} after {} completed step(s)",
            completed_steps.len()
        ),
        details,
    }
}

fn recovery_details(
    failed_step: &str,
    playlist_id: Option<&str>,
    cover: Option<CoverReference<'_>>,
) -> serde_json::Value {
    match (failed_step, playlist_id, cover) {
        ("cover_update", Some(playlist_id), Some(cover)) => serde_json::json!({
            "resumable": true,
            "command": "sunox playlist set",
            "arguments": {
                "playlist_id": playlist_id,
                "image_url": cover.image_url,
            }
        }),
        ("readback", Some(playlist_id), _) => serde_json::json!({
            "resumable": true,
            "command": "sunox playlist info",
            "arguments": { "playlist_id": playlist_id }
        }),
        ("metadata_update", Some(playlist_id), Some(cover)) if cover.uploaded_here => {
            serde_json::json!({
                "resumable": true,
                "command": "sunox playlist set",
                "arguments": {
                    "playlist_id": playlist_id,
                    "image_url": cover.image_url
                },
                "reuse_original_arguments": true,
                "omit_original_arguments": ["image_file"]
            })
        }
        ("metadata_update", Some(playlist_id), _) => serde_json::json!({
            "resumable": true,
            "command": "sunox playlist set",
            "arguments": { "playlist_id": playlist_id },
            "reuse_original_arguments": true
        }),
        ("playlist_create", None, Some(cover)) if cover.uploaded_here => serde_json::json!({
            "resumable": true,
            "command": "sunox playlist create",
            "arguments": { "image_url": cover.image_url },
            "reuse_original_arguments": true,
            "omit_original_arguments": ["image_file"]
        }),
        _ => serde_json::json!({ "resumable": false }),
    }
}
