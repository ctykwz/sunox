use std::collections::BTreeMap;

use reqwest::multipart::{Form, Part};
use tokio_util::io::ReaderStream;

use super::SunoClient;
use super::mutation::MutationSpec;
use super::types::{
    AudioUploadInitResponse, AudioUploadStatus, CreateAudioUploadRequest, CreateImageUploadRequest,
    FinishAudioUploadRequest, FinishImageUploadResponse, ImageUploadInitResponse,
    InitializeAudioClipRequest, InitializeAudioClipResponse,
};
use crate::core::CliError;
use crate::net::http;

impl SunoClient {
    /// Start Suno's presigned audio upload flow.
    pub async fn create_audio_upload(
        &self,
        req: &CreateAudioUploadRequest,
    ) -> Result<AudioUploadInitResponse, CliError> {
        let spec = upload_mutation_spec("audio_upload_create", None);
        let response: AudioUploadInitResponse = self
            .mutation_json_once(
                self.post_without_redirect("/api/uploads/audio/").json(req),
                &spec,
            )
            .await?;
        validate_upload_init(&spec, &response.id, &response.url)?;
        Ok(response)
    }

    /// Upload local bytes to the presigned S3 form returned by Suno.
    pub async fn upload_presigned_audio_file(
        &self,
        url: &str,
        fields: &BTreeMap<String, String>,
        filename: &str,
        file: tokio::fs::File,
        content_length: u64,
    ) -> Result<(), CliError> {
        let body = reqwest::Body::wrap_stream(ReaderStream::new(file));
        let part = Part::stream_with_length(body, content_length).file_name(filename.to_string());
        self.upload_presigned_part(url, fields, part).await
    }

    /// Start Suno's presigned image upload flow.
    pub async fn create_image_upload(
        &self,
        req: &CreateImageUploadRequest,
    ) -> Result<ImageUploadInitResponse, CliError> {
        let spec = upload_mutation_spec("image_upload_create", None);
        let response: ImageUploadInitResponse = self
            .mutation_json_once(
                self.post_without_redirect("/api/uploads/image/").json(req),
                &spec,
            )
            .await?;
        validate_upload_init(&spec, &response.id, &response.url)?;
        Ok(response)
    }

    /// Upload local image bytes to the presigned S3 form returned by Suno.
    pub async fn upload_presigned_image_form(
        &self,
        url: &str,
        fields: &BTreeMap<String, String>,
        filename: &str,
        content_type: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<(), CliError> {
        let mut part = Part::bytes(bytes).file_name(filename.to_string());
        if let Some(content_type) = content_type {
            part = part
                .mime_str(content_type)
                .map_err(|e| CliError::Config(format!("invalid upload content type: {e}")))?;
        }
        self.upload_presigned_part(url, fields, part).await
    }

    async fn upload_presigned_part(
        &self,
        url: &str,
        fields: &BTreeMap<String, String>,
        file_part: Part,
    ) -> Result<(), CliError> {
        let mut form = Form::new();
        for (key, value) in fields {
            form = form.text(key.clone(), value.clone());
        }
        form = form.part("file", file_part);

        let resp = http::transfer_client()?
            .post(url)
            .multipart(form)
            .send()
            .await?;
        self.check_response(resp).await?;
        Ok(())
    }

    /// Mark a presigned audio upload as finished after the S3 form upload.
    pub async fn finish_audio_upload(
        &self,
        upload_id: &str,
        req: &FinishAudioUploadRequest,
    ) -> Result<(), CliError> {
        let spec = upload_mutation_spec("audio_upload_finish", Some(upload_id));
        self.send_mutation_once(
            self.post_without_redirect(&format!("/api/uploads/audio/{upload_id}/upload-finish/"))
                .json(req),
            &spec,
        )
        .await?;
        Ok(())
    }

    /// Mark a presigned image upload as finished after the S3 form upload.
    pub async fn finish_image_upload(
        &self,
        upload_id: &str,
    ) -> Result<FinishImageUploadResponse, CliError> {
        let spec = upload_mutation_spec("image_upload_finish", Some(upload_id));
        self.mutation_json_once(
            self.post_without_redirect(&format!("/api/uploads/image/{upload_id}/upload-finish/"))
                .json(&serde_json::json!({})),
            &spec,
        )
        .await
    }

    /// Fetch Suno's processing status for an uploaded audio file.
    pub async fn get_audio_upload(&self, upload_id: &str) -> Result<AudioUploadStatus, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/uploads/audio/{upload_id}/"))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Initialize a library clip from a completed audio upload.
    pub async fn initialize_audio_clip(
        &self,
        upload_id: &str,
        req: &InitializeAudioClipRequest,
    ) -> Result<InitializeAudioClipResponse, CliError> {
        let spec = upload_mutation_spec("audio_upload_initialize_clip", Some(upload_id));
        self.mutation_json_once(
            self.post_without_redirect(&format!("/api/uploads/audio/{upload_id}/initialize-clip/"))
                .json(req),
            &spec,
        )
        .await
    }
}

fn upload_mutation_spec(operation: &'static str, upload_id: Option<&str>) -> MutationSpec {
    let resource = upload_id
        .map(|id| format!("upload {id}"))
        .unwrap_or_else(|| "new upload".to_string());
    let commands = upload_id
        .map(|id| vec![format!("sunox clip upload-status {id} --json")])
        .unwrap_or_default();
    let mut spec = MutationSpec::new(operation, resource, commands);
    if let Some(upload_id) = upload_id {
        spec = spec.with_context(
            "upload_id",
            serde_json::Value::String(upload_id.to_string()),
        );
    }
    spec
}

fn validate_upload_init(spec: &MutationSpec, id: &str, url: &str) -> Result<(), CliError> {
    if id.trim().is_empty() || url.trim().is_empty() {
        return Err(spec.ambiguous(
            "response_schema",
            "schema_drift",
            "upload creation returned a blank id or URL".into(),
        ));
    }
    Ok(())
}
