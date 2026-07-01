use std::path::Path;

use serde::Serialize;

use crate::api::SunoClient;
use crate::api::types::CreateImageUploadRequest;
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
    let content_type = upload_content_type(&upload.fields);

    client
        .upload_presigned_image_form(&upload.url, &upload.fields, &filename, content_type, bytes)
        .await?;

    let finish = client.finish_image_upload(&upload.id).await?;
    if finish.moderation_status.as_deref() != Some("approved") {
        return Err(CliError::Api {
            code: "image_moderation",
            message: format!(
                "image upload {} was not approved by Suno moderation: {}",
                upload.id,
                finish.moderation_status.as_deref().unwrap_or("unknown")
            ),
        });
    }

    let cover_image_s3_id = format!("image_{}", upload.id);
    Ok(ImageUploadResult {
        upload_id: upload.id,
        image_url: format!("https://cdn2.suno.ai/{cover_image_s3_id}.jpeg"),
        cover_image_s3_id,
        moderation_status: finish.moderation_status,
    })
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
