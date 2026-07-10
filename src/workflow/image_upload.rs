use std::path::Path;

use serde::Serialize;

use crate::api::SunoClient;
use crate::api::types::{CreateImageUploadRequest, SetMetadataRequest};
use crate::core::CliError;

use super::upload::upload_filename;

#[derive(Debug, Serialize)]
pub struct ImageUploadResult {
    pub upload_id: String,
    pub image_url: String,
    pub cover_image_s3_id: String,
    pub moderation_status: Option<String>,
}

pub async fn run(client: &SunoClient, file: &Path) -> Result<ImageUploadResult, CliError> {
    let extension = image_extension(file)?;
    let filename = upload_filename(file)?;
    let bytes = tokio::fs::read(file).await?;

    let upload = client
        .create_image_upload(&CreateImageUploadRequest { extension })
        .await?;
    let mut completed_steps = vec!["upload_created"];
    let content_type = upload_content_type(&upload.fields);

    client
        .upload_presigned_image_form(&upload.url, &upload.fields, &filename, content_type, bytes)
        .await
        .map_err(|error| {
            image_upload_stage_error(&upload.id, &completed_steps, "file_upload", error)
        })?;
    completed_steps.push("file_uploaded");

    let finish = client
        .finish_image_upload(&upload.id)
        .await
        .map_err(|error| {
            image_upload_stage_error(&upload.id, &completed_steps, "upload_finish", error)
        })?;
    completed_steps.push("upload_finished");
    if finish.moderation_status.as_deref() != Some("approved") {
        return Err(image_upload_stage_error(
            &upload.id,
            &completed_steps,
            "moderation",
            CliError::Api {
                code: "image_moderation",
                message: format!(
                    "image upload {} was not approved by Suno moderation: {}",
                    upload.id,
                    finish.moderation_status.as_deref().unwrap_or("unknown")
                ),
            },
        ));
    }

    let cover_image_s3_id = format!("image_{}", upload.id);
    Ok(ImageUploadResult {
        upload_id: upload.id,
        image_url: format!("https://cdn2.suno.ai/{cover_image_s3_id}.jpeg"),
        cover_image_s3_id,
        moderation_status: finish.moderation_status,
    })
}

pub async fn apply_uploaded_cover_to_clip(
    client: &SunoClient,
    clip_id: &str,
    request: &SetMetadataRequest,
    cover: &ImageUploadResult,
) -> Result<(), CliError> {
    client
        .set_metadata(clip_id, request)
        .await
        .map_err(|error| CliError::PartialMutation {
            message: format!(
                "clip_set for {clip_id} stopped at metadata_update after 1 completed step"
            ),
            details: serde_json::json!({
                "operation": "clip_set",
                "clip_id": clip_id,
                "cover": {
                    "upload_id": cover.upload_id,
                    "image_url": cover.image_url,
                    "uploaded_here": true
                },
                "completed_steps": ["cover_uploaded"],
                "failed": {
                    "step": "metadata_update",
                    "code": error.error_code(),
                    "message": error.to_string()
                },
                "recovery": {
                    "resumable": true,
                    "command": "sunox clip set",
                    "arguments": {
                        "clip_id": clip_id,
                        "image_url": cover.image_url
                    },
                    "reuse_original_arguments": true,
                    "omit_original_arguments": ["image_file"]
                }
            }),
        })
}

fn image_upload_stage_error(
    upload_id: &str,
    completed_steps: &[&str],
    failed_step: &str,
    error: CliError,
) -> CliError {
    let recovery = match failed_step {
        "file_upload" => serde_json::json!({
            "resumable": false,
            "reason": "the presigned image upload form cannot be safely reconstructed"
        }),
        "upload_finish" => serde_json::json!({
            "resumable": false,
            "reason": "retry safety for image upload finish is not live-verified"
        }),
        "moderation" => serde_json::json!({
            "resumable": false,
            "reason": "the uploaded image was not approved"
        }),
        _ => serde_json::json!({ "resumable": false }),
    };
    CliError::PartialMutation {
        message: format!(
            "image upload {upload_id} stopped at {failed_step} after {} completed step(s)",
            completed_steps.len()
        ),
        details: serde_json::json!({
            "operation": "image_upload",
            "upload_id": upload_id,
            "completed_steps": completed_steps,
            "failed": {
                "step": failed_step,
                "code": error.error_code(),
                "message": error.to_string()
            },
            "recovery": recovery
        }),
    }
}

pub fn image_extension(path: &Path) -> Result<String, CliError> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.trim_start_matches('.').to_ascii_lowercase())
        .filter(|extension| matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp"))
        .ok_or_else(|| {
            CliError::Config("image upload file must be png, jpg, jpeg, or webp".into())
        })?;
    Ok(extension)
}

fn upload_content_type(fields: &std::collections::BTreeMap<String, String>) -> Option<&str> {
    fields
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use std::collections::BTreeMap;

    use super::{image_extension, upload_content_type};

    #[test]
    fn image_extension_accepts_supported_images() {
        assert_eq!(
            image_extension(Path::new("/tmp/Cover.PNG")).expect("extension"),
            "png"
        );
        assert_eq!(
            image_extension(Path::new("/tmp/Cover.jpeg")).expect("extension"),
            "jpeg"
        );
    }

    #[test]
    fn image_extension_rejects_non_images() {
        let err = image_extension(Path::new("/tmp/Cover.txt")).expect_err("image extension");

        assert!(err.to_string().contains("png, jpg, jpeg, or webp"));
    }

    #[test]
    fn upload_content_type_reads_case_insensitive_s3_field() {
        let mut fields = BTreeMap::new();
        fields.insert("content-type".to_string(), "image/png".to_string());

        assert_eq!(upload_content_type(&fields), Some("image/png"));
    }
}
