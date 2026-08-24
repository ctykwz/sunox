use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::api::types::{
    CreatePersonaRequest, CreateVoiceVerificationRequest, PersonaInfo, ProcessVoiceSampleRequest,
    ProcessVoiceVerificationRecordingRequest, ProcessedVoiceStatus, VoiceVerification,
};
use crate::api::{PollingOptions, SunoClient};
use crate::core::{CliError, run_before_deadline, sleep_before_deadline};

use super::upload::{
    VoiceRecordingUploadInput, audio_extension, preflight_audio_file, upload_voice_recording_asset,
};

const VOX_MIN_SECONDS: f64 = 10.0;
const VOX_MAX_SECONDS: f64 = 240.0;
const VOX_SOURCE_MIN_SECONDS: f64 = 3.0;
const VOICE_UPLOAD_MAX_SECONDS: f64 = 900.0;
const PROCESSED_MAX_POLLS: usize = 120;
const VERIFICATION_MAX_POLLS: usize = 40;
const PERSONA_READBACK_MAX_POLLS: usize = 5;
const PROCESSED_POLL_INTERVAL: Duration = Duration::from_secs(1);
const VERIFICATION_POLL_INTERVAL: Duration = Duration::from_millis(1_500);
const PERSONA_READBACK_INTERVAL: Duration = Duration::from_secs(1);
const VOICE_NAME_MAX_UTF16: usize = 80;
const VOICE_STYLES_MAX_UTF16: usize = 256;
const VOICE_DESCRIPTION_MAX_UTF16: usize = 2_000;
const SINGER_SKILL_LEVELS: [&str; 4] = ["Beginner", "Intermediate", "Advanced", "Professional"];
const VOICE_LANGUAGES: [&str; 10] = ["en", "es", "fr", "pt", "de", "ja", "ko", "zh", "hi", "ru"];

pub struct VoiceCreateInput {
    pub sample_file: PathBuf,
    pub verification_file: PathBuf,
    pub phrase_id: String,
    pub language: String,
    pub sample_duration: f64,
    pub name: String,
    pub description: Option<String>,
    pub user_input_styles: Option<String>,
    pub singer_skill_level: Option<String>,
    pub confirm_rights: bool,
    pub confirm_eligibility: bool,
    pub confirm_biometric_consent: bool,
    /// Override used by tests and embedders. The CLI stores checkpoints in
    /// its managed config directory.
    pub checkpoint_dir: Option<PathBuf>,
    pub polling: PollingOptions,
}

#[derive(Debug, Serialize)]
pub struct VoiceCreateResult {
    pub workflow_id: String,
    pub phrase_id: String,
    pub sample_upload_id: String,
    pub sample_processed_id: String,
    pub sample_voice_recording_id: String,
    pub verification_upload_id: String,
    pub verification_processed_id: Option<String>,
    pub verification_voice_recording_id: String,
    pub verification: VoiceVerification,
    pub persona: PersonaInfo,
    pub private_readback: bool,
    pub checkpoint_path: PathBuf,
}

#[derive(Default, Serialize)]
struct VoiceCheckpoint {
    schema_version: u32,
    workflow_id: String,
    phrase_id: String,
    language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    checkpoint_path: Option<PathBuf>,
    completed_steps: Vec<&'static str>,
    sample_upload_id: Option<String>,
    sample_processed_id: Option<String>,
    sample_voice_recording_id: Option<String>,
    verification_upload_id: Option<String>,
    verification_processed_id: Option<String>,
    verification_voice_recording_id: Option<String>,
    verification_id: Option<String>,
    persona_id: Option<String>,
}

impl VoiceCheckpoint {
    fn identifiers(&self) -> serde_json::Value {
        serde_json::json!({
            "phrase_id": self.phrase_id,
            "sample_upload_id": self.sample_upload_id,
            "sample_processed_id": self.sample_processed_id,
            "sample_voice_recording_id": self.sample_voice_recording_id,
            "verification_upload_id": self.verification_upload_id,
            "verification_processed_id": self.verification_processed_id,
            "verification_voice_recording_id": self.verification_voice_recording_id,
            "verification_id": self.verification_id,
            "persona_id": self.persona_id,
        })
    }
}

/// Validate all local Voice inputs before authentication or the first account
/// write. The workflow repeats the inexpensive checks defensively when called
/// directly.
pub async fn preflight(input: &VoiceCreateInput) -> Result<(), CliError> {
    input.polling.validate()?;
    validate_text("--name", &input.name)?;
    validate_utf16_max("--name", input.name.trim(), VOICE_NAME_MAX_UTF16)?;
    if let Some(description) = input.description.as_deref() {
        validate_utf16_max(
            "--description",
            description.trim(),
            VOICE_DESCRIPTION_MAX_UTF16,
        )?;
    }
    if let Some(styles) = input.user_input_styles.as_deref() {
        validate_utf16_max("--styles", styles.trim(), VOICE_STYLES_MAX_UTF16)?;
    }
    validate_text("--phrase-id", &input.phrase_id)?;
    validate_language(&input.language)?;
    validate_singer_skill_level(input.singer_skill_level.as_deref())?;
    if !input.confirm_rights {
        return Err(CliError::Config(
            "Voice creation requires --confirm-rights to affirm that both recordings contain only your own voice and do not violate third-party rights"
                .into(),
        ));
    }
    if !input.confirm_eligibility {
        return Err(CliError::Config(
            "Voice creation requires --confirm-eligibility to affirm that you are at least 18 and Voice is available in your region; Suno remains authoritative"
                .into(),
        ));
    }
    if !input.confirm_biometric_consent {
        return Err(CliError::Config(
            "Voice creation requires --confirm-biometric-consent to explicitly consent to Suno collecting and processing the recordings as possible biometric data under its current Terms and Privacy Policy"
                .into(),
        ));
    }
    let sample_duration = preflight_voice_wav("--sample", &input.sample_file).await?;
    validate_sample_selection(input.sample_duration, sample_duration)?;
    preflight_voice_wav("--verification", &input.verification_file).await?;
    Ok(())
}

async fn preflight_voice_wav(flag: &str, path: &Path) -> Result<f64, CliError> {
    let extension = audio_extension(path)?;
    if extension != "wav" {
        return Err(CliError::Config(format!(
            "{flag} must be a WAV file: the current Voice Web flow converts recordings to WAV before voice_recording upload, and direct {extension} upload is not verified"
        )));
    }
    preflight_audio_file(path).await?;
    let duration = inspect_pcm_wave(path)?;
    if duration > VOICE_UPLOAD_MAX_SECONDS {
        return Err(CliError::Config(format!(
            "{flag} WAV is {duration:.2}s, exceeding the current Voice upload maximum of {VOICE_UPLOAD_MAX_SECONDS:.0}s"
        )));
    }
    Ok(duration)
}

fn inspect_pcm_wave(path: &Path) -> Result<f64, CliError> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    if file_len < 12 {
        return Err(invalid_wave(path, "header is shorter than 12 bytes"));
    }

    let mut header = [0_u8; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(invalid_wave(path, "missing RIFF/WAVE signature"));
    }
    let riff_end = u64::from(u32::from_le_bytes(
        header[4..8].try_into().expect("RIFF size"),
    ))
    .saturating_add(8);
    if riff_end > file_len {
        return Err(invalid_wave(path, "declared RIFF size exceeds the file"));
    }

    let mut byte_rate = None;
    let mut data_size = None;
    loop {
        let mut chunk_header = [0_u8; 8];
        let read = file.read(&mut chunk_header)?;
        if read == 0 {
            break;
        }
        if read != chunk_header.len() {
            return Err(invalid_wave(path, "truncated chunk header"));
        }
        let chunk_size = u64::from(u32::from_le_bytes(
            chunk_header[4..8].try_into().expect("chunk size"),
        ));
        let chunk_start = file.stream_position()?;
        let padded_size = chunk_size.saturating_add(chunk_size % 2);
        let chunk_end = chunk_start.saturating_add(padded_size);
        if chunk_end > file_len || chunk_end > riff_end {
            return Err(invalid_wave(
                path,
                "chunk extends beyond the declared RIFF data",
            ));
        }

        match &chunk_header[..4] {
            b"fmt " => {
                if chunk_size < 16 {
                    return Err(invalid_wave(path, "fmt chunk is shorter than 16 bytes"));
                }
                let mut format = [0_u8; 16];
                file.read_exact(&mut format)?;
                let audio_format = u16::from_le_bytes(format[0..2].try_into().expect("format"));
                let channels = u16::from_le_bytes(format[2..4].try_into().expect("channels"));
                let sample_rate = u32::from_le_bytes(format[4..8].try_into().expect("sample rate"));
                let rate = u32::from_le_bytes(format[8..12].try_into().expect("byte rate"));
                let block_align =
                    u16::from_le_bytes(format[12..14].try_into().expect("block align"));
                if !matches!(audio_format, 1 | 3 | 0xfffe)
                    || channels == 0
                    || sample_rate == 0
                    || rate == 0
                    || block_align == 0
                {
                    return Err(invalid_wave(
                        path,
                        "fmt chunk does not describe supported PCM/float audio",
                    ));
                }
                byte_rate = Some(rate);
            }
            b"data" => {
                if chunk_size == 0 {
                    return Err(invalid_wave(path, "data chunk is empty"));
                }
                data_size.get_or_insert(chunk_size);
            }
            _ => {}
        }
        file.seek(SeekFrom::Start(chunk_end))?;
    }

    let rate = byte_rate.ok_or_else(|| invalid_wave(path, "missing fmt chunk"))?;
    let data = data_size.ok_or_else(|| invalid_wave(path, "missing data chunk"))?;
    let duration = data as f64 / f64::from(rate);
    if !duration.is_finite() || duration <= 0.0 {
        return Err(invalid_wave(path, "audio duration is not positive"));
    }
    Ok(duration)
}

fn invalid_wave(path: &Path, reason: &str) -> CliError {
    CliError::Config(format!("invalid WAV file {}: {reason}", path.display()))
}

async fn validate_voice_plan_access(client: &SunoClient) -> Result<(), CliError> {
    let billing = client.billing_info().await?;
    let has_persona = billing
        .plan
        .usage_plan_features
        .iter()
        .any(|feature| feature.name == "persona")
        || billing
            .accessible_features
            .as_ref()
            .is_some_and(|features| features.contains("persona"));
    if !billing.is_active || !has_persona {
        return Err(CliError::Config(
            "the current active Suno plan does not expose the `persona` feature required by the Web Voice flow"
                .into(),
        ));
    }
    Ok(())
}

fn resolve_checkpoint_path(
    workflow_id: &str,
    override_dir: Option<&Path>,
) -> Result<PathBuf, CliError> {
    let directory = override_dir
        .map(Path::to_path_buf)
        .or_else(|| crate::core::project_config_dir().map(|path| path.join("voice-checkpoints")));
    let directory = directory.ok_or_else(|| {
        CliError::Config(
            "could not resolve a managed config directory for the Voice workflow checkpoint".into(),
        )
    })?;
    Ok(directory.join(format!("{workflow_id}.json")))
}

fn record_checkpoint_step(
    checkpoint: &mut VoiceCheckpoint,
    path: &Path,
    step: &'static str,
) -> Result<(), CliError> {
    checkpoint.completed_steps.push(step);
    persist_checkpoint_at(path, checkpoint)
}

fn persist_checkpoint_at(path: &Path, checkpoint: &VoiceCheckpoint) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::Config("Voice checkpoint path has no parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }

    let body = serde_json::to_vec_pretty(checkpoint)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    temporary.write_all(&body)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| CliError::Io(error.error))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

pub async fn run(
    client: &SunoClient,
    input: VoiceCreateInput,
) -> Result<VoiceCreateResult, CliError> {
    preflight(&input).await?;
    validate_voice_plan_access(client).await?;
    let current_phrase = client.get_voice_phrase(input.language.trim()).await?;
    if current_phrase.phrase_id.trim().is_empty() || current_phrase.phrase_text.trim().is_empty() {
        return Err(CliError::Api {
            code: "schema_drift",
            message: "Voice phrase response omitted its phrase ID or text".into(),
        });
    }
    if current_phrase.phrase_id != input.phrase_id.trim() {
        return Err(CliError::Config(format!(
            "Voice phrase `{}` is no longer current for language `{}`; fetch and record the new phrase before creating a Voice",
            input.phrase_id.trim(),
            input.language.trim()
        )));
    }

    let workflow_id = uuid::Uuid::new_v4().to_string();
    let checkpoint_path = resolve_checkpoint_path(&workflow_id, input.checkpoint_dir.as_deref())?;
    let mut checkpoint = VoiceCheckpoint {
        schema_version: 1,
        workflow_id: workflow_id.clone(),
        phrase_id: input.phrase_id.trim().to_string(),
        language: input.language.trim().to_string(),
        checkpoint_path: Some(checkpoint_path.clone()),
        ..VoiceCheckpoint::default()
    };
    persist_checkpoint_at(&checkpoint_path, &checkpoint)?;
    let duration = round_hundredths(input.sample_duration);

    let sample_upload_result = upload_voice_recording_asset(
        client,
        VoiceRecordingUploadInput {
            file: &input.sample_file,
            workflow_id: &workflow_id,
            role: "sample",
            timeout: input.polling.timeout,
            poll_interval: input.polling.interval,
        },
        |upload_id| {
            checkpoint.sample_upload_id = Some(upload_id.to_string());
            record_checkpoint_step(&mut checkpoint, &checkpoint_path, "sample_upload_created")
        },
    )
    .await;
    let sample_upload = sample_upload_result
        .map_err(|error| stage_error(error, &checkpoint, "sample_upload", None))?;
    record_checkpoint_step(&mut checkpoint, &checkpoint_path, "sample_upload_complete")
        .map_err(|error| stage_error(error, &checkpoint, "sample_upload_checkpoint", None))?;

    let sample_processed = client
        .process_voice_sample(
            &workflow_id,
            &ProcessVoiceSampleRequest {
                upload_id: sample_upload.upload_id.clone(),
                vocal_start_s: 0.0,
                vocal_end_s: duration,
            },
        )
        .await
        .map_err(|error| stage_error(error, &checkpoint, "sample_process", None))?;
    checkpoint.sample_processed_id = Some(sample_processed.id.clone());
    checkpoint.sample_voice_recording_id = Some(sample_processed.voice_recording_id.clone());
    record_checkpoint_step(
        &mut checkpoint,
        &checkpoint_path,
        "sample_process_submitted",
    )
    .map_err(|error| stage_error(error, &checkpoint, "sample_process_checkpoint", None))?;

    wait_for_processed_voice(client, &sample_processed.id, input.polling)
        .await
        .map_err(|error| {
            stage_error(
                error,
                &checkpoint,
                "sample_processing_wait",
                Some(serde_json::json!({
                    "resumable": false,
                    "reason": "processed-status can inspect the sample, but the remaining multi-write Voice workflow has no verified resume command",
                    "inspection": {
                        "command": "sunox voice processed-status",
                        "arguments": { "processed_id": sample_processed.id }
                    }
                })),
            )
        })?;
    record_checkpoint_step(
        &mut checkpoint,
        &checkpoint_path,
        "sample_processing_complete",
    )
    .map_err(|error| stage_error(error, &checkpoint, "sample_processing_checkpoint", None))?;

    let verification_upload_result = upload_voice_recording_asset(
        client,
        VoiceRecordingUploadInput {
            file: &input.verification_file,
            workflow_id: &workflow_id,
            role: "verification",
            timeout: input.polling.timeout,
            poll_interval: input.polling.interval,
        },
        |upload_id| {
            checkpoint.verification_upload_id = Some(upload_id.to_string());
            record_checkpoint_step(
                &mut checkpoint,
                &checkpoint_path,
                "verification_upload_created",
            )
        },
    )
    .await;
    let verification_upload = verification_upload_result
        .map_err(|error| stage_error(error, &checkpoint, "verification_upload", None))?;
    record_checkpoint_step(
        &mut checkpoint,
        &checkpoint_path,
        "verification_upload_complete",
    )
    .map_err(|error| stage_error(error, &checkpoint, "verification_upload_checkpoint", None))?;

    let verification_recording = client
        .process_voice_verification_recording(
            &workflow_id,
            &ProcessVoiceVerificationRecordingRequest::new(verification_upload.upload_id.clone()),
        )
        .await
        .map_err(|error| stage_error(error, &checkpoint, "verification_recording_process", None))?;
    checkpoint.verification_voice_recording_id =
        Some(verification_recording.voice_recording_id.clone());
    checkpoint.verification_processed_id = verification_recording.id.clone();
    record_checkpoint_step(
        &mut checkpoint,
        &checkpoint_path,
        "verification_recording_processed",
    )
    .map_err(|error| {
        stage_error(
            error,
            &checkpoint,
            "verification_recording_checkpoint",
            None,
        )
    })?;

    let verification = client
        .create_voice_verification(
            &workflow_id,
            &CreateVoiceVerificationRequest {
                voice_recording_id: sample_processed.voice_recording_id.clone(),
                verification_recording_id: verification_recording.voice_recording_id.clone(),
                phrase_id: input.phrase_id.trim().to_string(),
            },
        )
        .await
        .map_err(|error| stage_error(error, &checkpoint, "verification_create", None))?;
    checkpoint.verification_id = Some(verification.id.clone());
    record_checkpoint_step(&mut checkpoint, &checkpoint_path, "verification_created")
        .map_err(|error| stage_error(error, &checkpoint, "verification_checkpoint", None))?;

    let verification = wait_for_voice_verification(client, verification, input.polling)
        .await
        .map_err(|error| {
            stage_error(
                error,
                &checkpoint,
                "verification_wait",
                Some(serde_json::json!({
                    "resumable": false,
                    "reason": "verification-status can inspect the result, but the private Voice persona creation has no verified resume command",
                    "inspection": {
                        "command": "sunox voice verification-status",
                        "arguments": { "verification_id": checkpoint.verification_id }
                    }
                })),
            )
        })?;
    if verification.status != "approved" {
        let reason = verification
            .rejection_reason
            .as_deref()
            .unwrap_or("unspecified");
        return Err(stage_error(
            CliError::GenerationFailed(format!(
                "Voice verification {} ended with status `{}` ({reason})",
                verification.id, verification.status
            )),
            &checkpoint,
            "verification_rejected",
            Some(serde_json::json!({
                "resumable": false,
                "reason": "a rejected Voice verification requires a new dynamic phrase recording",
                "verification_status": verification.status,
                "rejection_reason": verification.rejection_reason
            })),
        ));
    }
    record_checkpoint_step(&mut checkpoint, &checkpoint_path, "verification_approved").map_err(
        |error| stage_error(error, &checkpoint, "verification_approved_checkpoint", None),
    )?;

    let request = build_persona_request(&input, &sample_processed, &verification, duration);
    let created = client
        .create_verified_voice_persona(&workflow_id, &request)
        .await
        .map_err(|error| stage_error(error, &checkpoint, "persona_create", None))?;
    checkpoint.persona_id = Some(created.id.clone());
    record_checkpoint_step(&mut checkpoint, &checkpoint_path, "persona_created")
        .map_err(|error| stage_error(error, &checkpoint, "persona_checkpoint", None))?;

    let persona = wait_for_private_vox_persona(client, &created.id, input.polling)
        .await
        .map_err(|error| {
            stage_error(
                error,
                &checkpoint,
                "persona_readback",
                Some(serde_json::json!({
                    "resumable": false,
                    "reason": "persona info can inspect the created resource, but it cannot replay or prove the failed readback",
                    "inspection": {
                        "command": "sunox persona info",
                        "arguments": { "id": created.id }
                    }
                })),
            )
        })?;
    if persona.id != created.id {
        return Err(stage_error(
            CliError::Api {
                code: "schema_drift",
                message: format!(
                    "Voice persona readback returned `{}` instead of `{}`",
                    persona.id, created.id
                ),
            },
            &checkpoint,
            "persona_readback_identity",
            None,
        ));
    }
    if persona.is_public != Some(false) {
        let recovery = if persona.is_public == Some(true) {
            serde_json::json!({
                "resumable": true,
                "command": "sunox persona unpublish",
                "arguments": { "id": persona.id }
            })
        } else {
            serde_json::json!({
                "resumable": false,
                "reason": "the persona readback omitted its privacy state; inspect it before deciding on any write",
                "inspection": {
                    "command": "sunox persona info",
                    "arguments": { "id": persona.id }
                }
            })
        };
        return Err(stage_error(
            CliError::Api {
                code: "state_mismatch",
                message: format!(
                    "Voice persona {} was requested private but did not read back with is_public=false",
                    persona.id,
                ),
            },
            &checkpoint,
            "persona_private_readback",
            Some(recovery),
        ));
    }
    let is_vox = persona.is_vox_persona();
    if !is_vox {
        return Err(stage_error(
            CliError::Api {
                code: "state_mismatch",
                message: format!(
                    "persona {} was created but did not read back as a vox Voice",
                    persona.id
                ),
            },
            &checkpoint,
            "persona_type_readback",
            Some(serde_json::json!({
                "resumable": false,
                "reason": "persona info can inspect the resource but cannot change its type",
                "inspection": {
                    "command": "sunox persona info",
                    "arguments": { "id": persona.id }
                }
            })),
        ));
    }
    record_checkpoint_step(&mut checkpoint, &checkpoint_path, "persona_readback")
        .map_err(|error| stage_error(error, &checkpoint, "persona_readback_checkpoint", None))?;

    Ok(VoiceCreateResult {
        workflow_id,
        phrase_id: input.phrase_id.trim().to_string(),
        sample_upload_id: sample_upload.upload_id,
        sample_processed_id: sample_processed.id,
        sample_voice_recording_id: sample_processed.voice_recording_id,
        verification_upload_id: verification_upload.upload_id,
        verification_processed_id: verification_recording.id,
        verification_voice_recording_id: verification_recording.voice_recording_id,
        verification,
        private_readback: true,
        persona,
        checkpoint_path,
    })
}

fn build_persona_request(
    input: &VoiceCreateInput,
    sample: &crate::api::types::ProcessVoiceSampleResponse,
    verification: &VoiceVerification,
    duration: f64,
) -> CreatePersonaRequest {
    CreatePersonaRequest {
        root_clip_id: None,
        name: Some(input.name.trim().to_string()),
        description: Some(
            input
                .description
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .to_string(),
        ),
        image_s3_id: None,
        is_public: Some(false),
        is_suno_persona: None,
        persona_type: Some("vox".into()),
        vox_audio_id: Some(sample.id.clone()),
        vocal_start_s: Some(0.0),
        vocal_end_s: Some(duration),
        user_input_styles: trimmed_optional(input.user_input_styles.as_deref()),
        source: Some("random_song".into()),
        singer_skill_level: trimmed_optional(input.singer_skill_level.as_deref()),
        clips: None,
        is_voice_recording: Some(true),
        voice_recording_id: Some(sample.voice_recording_id.clone()),
        verification_id: Some(verification.id.clone()),
    }
}

async fn wait_for_processed_voice(
    client: &SunoClient,
    processed_id: &str,
    polling: PollingOptions,
) -> Result<ProcessedVoiceStatus, CliError> {
    let effective = bounded_polling(polling, PROCESSED_POLL_INTERVAL, PROCESSED_MAX_POLLS);
    let deadline = effective.deadline()?;
    for attempt in 0..PROCESSED_MAX_POLLS {
        let timeout_error = || processing_timeout(processed_id, polling.timeout);
        let status = run_before_deadline(
            deadline,
            client.get_processed_voice_status(processed_id),
            timeout_error(),
        )
        .await?;
        match status.status.as_str() {
            "complete" | "completed" => return Ok(status),
            "error" | "failed" => {
                return Err(CliError::GenerationFailed(format!(
                    "Voice sample processing {processed_id} failed"
                )));
            }
            _ if attempt + 1 == PROCESSED_MAX_POLLS
                || !sleep_before_deadline(deadline, effective.interval).await =>
            {
                return Err(timeout_error());
            }
            _ => {}
        }
    }
    unreachable!("bounded Voice processing loop returns on every terminal branch")
}

async fn wait_for_voice_verification(
    client: &SunoClient,
    initial: VoiceVerification,
    polling: PollingOptions,
) -> Result<VoiceVerification, CliError> {
    if initial.status != "pending" {
        return Ok(initial);
    }
    let verification_id = initial.id.clone();
    let effective = bounded_polling(polling, VERIFICATION_POLL_INTERVAL, VERIFICATION_MAX_POLLS);
    let deadline = effective.deadline()?;
    for _ in 0..VERIFICATION_MAX_POLLS {
        if !sleep_before_deadline(deadline, effective.interval).await {
            return Err(verification_timeout(&verification_id, polling.timeout));
        }
        let current = run_before_deadline(
            deadline,
            client.get_voice_verification(&verification_id),
            verification_timeout(&verification_id, polling.timeout),
        )
        .await?;
        if current.id != verification_id {
            return Err(CliError::Api {
                code: "schema_drift",
                message: format!(
                    "Voice verification readback returned `{}` while polling `{verification_id}`",
                    current.id
                ),
            });
        }
        if current.status != "pending" {
            return Ok(current);
        }
    }
    Err(verification_timeout(&verification_id, polling.timeout))
}

async fn wait_for_private_vox_persona(
    client: &SunoClient,
    persona_id: &str,
    polling: PollingOptions,
) -> Result<PersonaInfo, CliError> {
    let effective = bounded_polling(
        polling,
        PERSONA_READBACK_INTERVAL,
        PERSONA_READBACK_MAX_POLLS,
    );
    let deadline = effective.deadline()?;
    let mut last_persona = None;
    let mut last_not_found = None;
    for attempt in 0..PERSONA_READBACK_MAX_POLLS {
        match run_before_deadline(
            deadline,
            client.get_persona(persona_id),
            persona_readback_timeout(persona_id),
        )
        .await
        {
            Ok(persona) if persona.id != persona_id => {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "Voice persona readback returned `{}` instead of `{persona_id}`",
                        persona.id
                    ),
                });
            }
            Ok(persona) if persona.is_public == Some(false) && persona.is_vox_persona() => {
                return Ok(persona);
            }
            Ok(persona) => last_persona = Some(persona),
            Err(error)
                if matches!(
                    &error,
                    CliError::NotFound(_) | CliError::SunoApi { status: 404, .. }
                ) =>
            {
                last_not_found = Some(error)
            }
            Err(error) => return Err(error),
        }
        if attempt + 1 == PERSONA_READBACK_MAX_POLLS
            || !sleep_before_deadline(deadline, effective.interval).await
        {
            break;
        }
    }
    if let Some(persona) = last_persona {
        return Ok(persona);
    }
    Err(last_not_found.unwrap_or_else(|| persona_readback_timeout(persona_id)))
}

fn bounded_polling(
    configured: PollingOptions,
    web_interval: Duration,
    max_polls: usize,
) -> PollingOptions {
    let interval = configured.interval.max(web_interval);
    let web_timeout = web_interval.saturating_mul(max_polls as u32);
    PollingOptions {
        timeout: configured.timeout.min(web_timeout),
        interval,
    }
}

fn persona_readback_timeout(persona_id: &str) -> CliError {
    CliError::GenerationFailed(format!(
        "Voice persona {persona_id} did not converge to a private vox readback within {PERSONA_READBACK_MAX_POLLS} polls"
    ))
}

fn processing_timeout(processed_id: &str, timeout: Duration) -> CliError {
    CliError::GenerationFailed(format!(
        "Voice sample processing {processed_id} did not complete within {} seconds or {PROCESSED_MAX_POLLS} polls",
        timeout
            .min(PROCESSED_POLL_INTERVAL.saturating_mul(PROCESSED_MAX_POLLS as u32))
            .as_secs()
    ))
}

fn verification_timeout(verification_id: &str, timeout: Duration) -> CliError {
    CliError::GenerationFailed(format!(
        "Voice verification {verification_id} did not complete within {} seconds or {VERIFICATION_MAX_POLLS} polls",
        timeout
            .min(VERIFICATION_POLL_INTERVAL.saturating_mul(VERIFICATION_MAX_POLLS as u32))
            .as_secs()
    ))
}

fn stage_error(
    mut error: CliError,
    checkpoint: &VoiceCheckpoint,
    failed_step: &str,
    recovery: Option<serde_json::Value>,
) -> CliError {
    match &mut error {
        CliError::AmbiguousMutation { details, .. } | CliError::PartialMutation { details, .. } => {
            if let Some(details) = details.as_object_mut() {
                let mut identifiers = checkpoint.identifiers();
                if let (Some(role), Some(upload_id)) = (
                    details
                        .get("recording_role")
                        .and_then(|value| value.as_str()),
                    details.get("upload_id").cloned(),
                ) && let Some(identifiers) = identifiers.as_object_mut()
                {
                    let key = match role {
                        "sample" => Some("sample_upload_id"),
                        "verification" => Some("verification_upload_id"),
                        _ => None,
                    };
                    if let Some(key) = key {
                        identifiers.insert(key.into(), upload_id);
                    }
                }
                details.insert(
                    "workflow_id".into(),
                    serde_json::json!(checkpoint.workflow_id),
                );
                if let Some(path) = &checkpoint.checkpoint_path {
                    details.insert("workflow_checkpoint_path".into(), serde_json::json!(path));
                }
                details.insert("identifiers".into(), identifiers);
                details.insert(
                    "workflow_completed_steps".into(),
                    serde_json::json!(checkpoint.completed_steps),
                );
                details.insert(
                    "workflow_failed_step".into(),
                    serde_json::json!(failed_step),
                );
                if let Some(recovery) = recovery {
                    details.insert("workflow_recovery".into(), recovery);
                }
            }
            return error;
        }
        _ if checkpoint.completed_steps.is_empty() => return error,
        _ => {}
    }

    CliError::PartialMutation {
        message: format!(
            "Voice workflow {} stopped at {failed_step} after {} completed stage(s)",
            checkpoint.workflow_id,
            checkpoint.completed_steps.len()
        ),
        details: serde_json::json!({
            "operation": "voice_create",
            "workflow_id": checkpoint.workflow_id,
            "workflow_checkpoint_path": checkpoint.checkpoint_path,
            "completed_steps": checkpoint.completed_steps,
            "identifiers": checkpoint.identifiers(),
            "failed": {
                "step": failed_step,
                "code": error.error_code(),
                "message": error.to_string(),
                "details": error.details()
            },
            "recovery": recovery.unwrap_or_else(|| serde_json::json!({
                "resumable": false,
                "reason": "the failed Voice stage has no verified idempotent resume operation"
            }))
        }),
    }
}

fn validate_text(flag: &str, value: &str) -> Result<(), CliError> {
    if value.trim().is_empty() {
        return Err(CliError::Config(format!("{flag} cannot be blank")));
    }
    Ok(())
}

fn validate_utf16_max(flag: &str, value: &str, maximum: usize) -> Result<(), CliError> {
    let units = value.encode_utf16().count();
    if units <= maximum {
        return Ok(());
    }
    Err(CliError::Config(format!(
        "{flag} contains {units} UTF-16 code units; current Suno Web allows at most {maximum}"
    )))
}

fn validate_sample_selection(duration: f64, source_duration: f64) -> Result<(), CliError> {
    if !source_duration.is_finite() || source_duration < VOX_SOURCE_MIN_SECONDS {
        return Err(CliError::Config(format!(
            "--sample WAV must contain at least {VOX_SOURCE_MIN_SECONDS:.0} seconds of audio"
        )));
    }
    let wire_duration = round_hundredths(duration);
    let source_wire_duration = round_hundredths(source_duration);
    let minimum = VOX_MIN_SECONDS.min(source_wire_duration);
    let maximum = VOX_MAX_SECONDS.min(source_wire_duration);
    if !duration.is_finite() || wire_duration < minimum || wire_duration > maximum {
        return Err(CliError::Config(format!(
            "--sample-duration rounds to {wire_duration:.2}s and must be between {minimum:.2} and {maximum:.2} seconds for this {source_duration:.2}s WAV"
        )));
    }
    if wire_duration != source_wire_duration {
        return Err(CliError::Config(format!(
            "--sample WAV must already be trimmed to the selected duration before upload: file rounds to {source_wire_duration:.2}s but --sample-duration rounds to {wire_duration:.2}s"
        )));
    }
    Ok(())
}

fn validate_language(value: &str) -> Result<(), CliError> {
    if VOICE_LANGUAGES.contains(&value.trim()) {
        return Ok(());
    }
    Err(CliError::Config(format!(
        "--language must be one of: {}",
        VOICE_LANGUAGES.join(", ")
    )))
}

fn validate_singer_skill_level(value: Option<&str>) -> Result<(), CliError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if SINGER_SKILL_LEVELS.contains(&value) {
        return Ok(());
    }
    Err(CliError::Config(format!(
        "--singer-skill-level must be one of: {}",
        SINGER_SKILL_LEVELS.join(", ")
    )))
}

fn round_hundredths(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn trimmed_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::oneshot;

    use crate::api::PollingOptions;
    use crate::api::types::{ProcessVoiceSampleResponse, VoiceVerification};
    use crate::auth::AuthState;
    use crate::core::CliError;

    use super::{
        PROCESSED_MAX_POLLS, PROCESSED_POLL_INTERVAL, VERIFICATION_MAX_POLLS,
        VERIFICATION_POLL_INTERVAL, VoiceCheckpoint, VoiceCreateInput, bounded_polling,
        build_persona_request, inspect_pcm_wave, persist_checkpoint_at, preflight,
        round_hundredths, run, stage_error, validate_sample_selection, validate_singer_skill_level,
        validate_utf16_max, wait_for_private_vox_persona, wait_for_voice_verification,
    };

    struct CapturedRequest {
        method: String,
        path: String,
        body: String,
    }

    async fn mock_sequence(
        responses: Vec<String>,
    ) -> (String, oneshot::Receiver<Vec<CapturedRequest>>) {
        mock_status_sequence(responses.into_iter().map(|body| (200_u16, body)).collect()).await
    }

    async fn mock_status_sequence(
        responses: Vec<(u16, String)>,
    ) -> (String, oneshot::Receiver<Vec<CapturedRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock server");
        let address = listener.local_addr().expect("mock address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let mut requests = Vec::with_capacity(responses.len());
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.expect("accept mock request");
                requests.push(read_request(&mut stream).await);
                let response = format!(
                    "HTTP/1.1 {status} Test\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream
                    .write_all(response.as_bytes())
                    .await
                    .expect("write mock response");
            }
            let _ = sender.send(requests);
        });
        (format!("http://{address}"), receiver)
    }

    async fn read_request(stream: &mut TcpStream) -> CapturedRequest {
        let mut data = Vec::new();
        let mut buffer = [0_u8; 2048];
        let header_end = loop {
            let read = stream
                .read(&mut buffer)
                .await
                .expect("read request headers");
            assert_ne!(read, 0, "request ended before headers");
            data.extend_from_slice(&buffer[..read]);
            if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = String::from_utf8_lossy(&data[..header_end]);
        let request_line = headers.lines().next().expect("request line");
        let mut parts = request_line.split_whitespace();
        let method = parts.next().expect("method").to_string();
        let path = parts.next().expect("path").to_string();
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(str::to_string)
            })
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        while data.len() < header_end + content_length {
            let read = stream.read(&mut buffer).await.expect("read request body");
            assert_ne!(read, 0, "request ended before body");
            data.extend_from_slice(&buffer[..read]);
        }
        CapturedRequest {
            method,
            path,
            body: String::from_utf8_lossy(&data[header_end..header_end + content_length])
                .to_string(),
        }
    }

    fn input() -> VoiceCreateInput {
        VoiceCreateInput {
            sample_file: PathBuf::from("sample.wav"),
            verification_file: PathBuf::from("verify.wav"),
            phrase_id: "phrase-1".into(),
            language: "en".into(),
            sample_duration: 31.456,
            name: " My Voice ".into(),
            description: None,
            user_input_styles: Some(" warm soul ".into()),
            singer_skill_level: None,
            confirm_rights: true,
            confirm_eligibility: true,
            confirm_biometric_consent: true,
            checkpoint_dir: None,
            polling: PollingOptions {
                timeout: Duration::from_secs(60),
                interval: Duration::from_secs(1),
            },
        }
    }

    fn write_pcm_wav(path: &std::path::Path, seconds: u32) {
        let sample_rate = 8_000_u32;
        let data_len = sample_rate.checked_mul(seconds).expect("fixture duration");
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize(44 + data_len as usize, 128);
        std::fs::write(path, wav).expect("WAV fixture");
    }

    #[test]
    fn sample_duration_matches_current_voice_trim_limits() {
        validate_sample_selection(10.0, 10.0).expect("minimum for a long source");
        validate_sample_selection(240.0, 240.0).expect("maximum");
        validate_sample_selection(5.0, 5.0).expect("short sources select the full file");
        for invalid in [9.99, 240.01, f64::NAN, f64::INFINITY] {
            let error = validate_sample_selection(invalid, 900.0).expect_err("invalid duration");
            assert!(matches!(error, CliError::Config(_)));
        }
        validate_sample_selection(4.99, 5.0).expect_err("short source must be selected in full");
        validate_sample_selection(5.0, 5.003).expect("wire-rounded short source duration");
        validate_sample_selection(240.0, 239.999).expect("wire-rounded maximum duration");
        validate_sample_selection(30.0, 60.0)
            .expect_err("the CLI must not upload untrimmed extra audio");
        validate_sample_selection(2.9, 2.9).expect_err("source must reach the Web minimum");
        assert_eq!(round_hundredths(31.456), 31.46);
    }

    #[test]
    fn voice_polling_cannot_exceed_current_web_budgets() {
        let configured = PollingOptions {
            timeout: Duration::from_secs(600),
            interval: Duration::from_millis(100),
        };
        let processing = bounded_polling(configured, PROCESSED_POLL_INTERVAL, PROCESSED_MAX_POLLS);
        assert_eq!(processing.interval, Duration::from_secs(1));
        assert_eq!(processing.timeout, Duration::from_secs(120));

        let verification = bounded_polling(
            configured,
            VERIFICATION_POLL_INTERVAL,
            VERIFICATION_MAX_POLLS,
        );
        assert_eq!(verification.interval, Duration::from_millis(1_500));
        assert_eq!(verification.timeout, Duration::from_secs(60));

        let deliberately_slower = bounded_polling(
            PollingOptions {
                timeout: Duration::from_secs(600),
                interval: Duration::from_secs(5),
            },
            PROCESSED_POLL_INTERVAL,
            PROCESSED_MAX_POLLS,
        );
        assert_eq!(deliberately_slower.interval, Duration::from_secs(5));
    }

    #[test]
    fn singer_skill_level_matches_current_web_choices() {
        for level in ["Beginner", "Intermediate", "Advanced", "Professional"] {
            validate_singer_skill_level(Some(level)).expect("current Web choice");
        }
        validate_singer_skill_level(None).expect("skipping the field is supported");
        validate_singer_skill_level(Some("advanced")).expect_err("values are protocol-cased");
    }

    #[test]
    fn voice_text_limits_match_html_utf16_max_length_semantics() {
        validate_utf16_max("--name", &"a".repeat(80), 80).expect("80 ASCII units");
        validate_utf16_max("--name", &"😀".repeat(40), 80).expect("40 surrogate pairs");
        let error = validate_utf16_max("--name", &"😀".repeat(41), 80)
            .expect_err("41 surrogate pairs exceed 80 units");
        assert!(error.to_string().contains("82 UTF-16"));
    }

    #[tokio::test]
    async fn voice_preflight_rejects_unverified_non_wav_uploads() {
        let directory = tempfile::tempdir().expect("temp directory");
        let sample = directory.path().join("sample.mp3");
        let verification = directory.path().join("verification.wav");
        std::fs::write(&sample, b"sample fixture").expect("sample fixture");
        std::fs::write(&verification, b"verification fixture").expect("verification fixture");
        let mut input = input();
        input.sample_file = sample;
        input.verification_file = verification;

        let error = preflight(&input)
            .await
            .expect_err("non-WAV Voice uploads are not verified");

        assert!(
            matches!(error, CliError::Config(message) if message.contains("must be a WAV file"))
        );
    }

    #[tokio::test]
    async fn voice_preflight_validates_wave_structure_and_selected_duration() {
        let directory = tempfile::tempdir().expect("temp directory");
        let sample = directory.path().join("sample.wav");
        let verification = directory.path().join("verification.wav");
        std::fs::write(&sample, b"not really a WAV").expect("invalid fixture");
        write_pcm_wav(&verification, 15);
        let mut input = input();
        input.sample_file = sample.clone();
        input.verification_file = verification.clone();

        let error = preflight(&input)
            .await
            .expect_err("invalid header must fail");
        assert!(error.to_string().contains("RIFF/WAVE"));

        write_pcm_wav(&sample, 20);
        input.sample_duration = 21.0;
        let error = preflight(&input)
            .await
            .expect_err("selection cannot exceed actual audio");
        assert!(error.to_string().contains("must be between"));
        assert_eq!(inspect_pcm_wave(&verification).expect("duration"), 15.0);
    }

    #[test]
    fn voice_checkpoint_is_written_atomically_without_recording_secrets() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("workflow.json");
        let checkpoint = VoiceCheckpoint {
            schema_version: 1,
            workflow_id: "workflow-1".into(),
            phrase_id: "phrase-1".into(),
            language: "en".into(),
            checkpoint_path: Some(path.clone()),
            completed_steps: vec!["sample_upload_created"],
            sample_upload_id: Some("upload-main".into()),
            ..VoiceCheckpoint::default()
        };

        persist_checkpoint_at(&path, &checkpoint).expect("persist checkpoint");

        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read checkpoint"))
                .expect("checkpoint JSON");
        assert_eq!(saved["schema_version"], 1);
        assert_eq!(saved["sample_upload_id"], "upload-main");
        assert!(saved.get("jwt").is_none());
        assert!(saved.get("phrase_text").is_none());
    }

    #[test]
    fn final_persona_body_is_private_verified_vox() {
        let input = input();
        let sample = ProcessVoiceSampleResponse {
            id: "processed-1".into(),
            voice_recording_id: "recording-main".into(),
            extra: Default::default(),
        };
        let verification = VoiceVerification {
            id: "verification-1".into(),
            status: "approved".into(),
            rejection_reason: None,
            extra: Default::default(),
        };

        let request = build_persona_request(&input, &sample, &verification, 31.46);
        let body = serde_json::to_value(request).expect("serialize persona request");

        assert_eq!(
            body,
            serde_json::json!({
                "name": "My Voice",
                "description": "",
                "is_public": false,
                "persona_type": "vox",
                "vox_audio_id": "processed-1",
                "vocal_start_s": 0.0,
                "vocal_end_s": 31.46,
                "user_input_styles": "warm soul",
                "source": "random_song",
                "is_voice_recording": true,
                "voice_recording_id": "recording-main",
                "verification_id": "verification-1"
            })
        );
    }

    #[test]
    fn partial_failure_keeps_every_durable_voice_identity() {
        let checkpoint = VoiceCheckpoint {
            schema_version: 1,
            workflow_id: "workflow-1".into(),
            phrase_id: "phrase-1".into(),
            language: "en".into(),
            checkpoint_path: Some(PathBuf::from("/tmp/workflow-1.json")),
            completed_steps: vec![
                "sample_upload_complete",
                "sample_process_submitted",
                "sample_processing_complete",
                "verification_upload_complete",
                "verification_recording_processed",
                "verification_created",
            ],
            sample_upload_id: Some("upload-main".into()),
            sample_processed_id: Some("processed-main".into()),
            sample_voice_recording_id: Some("recording-main".into()),
            verification_upload_id: Some("upload-verify".into()),
            verification_processed_id: Some("processed-verify".into()),
            verification_voice_recording_id: Some("recording-verify".into()),
            verification_id: Some("verification-1".into()),
            persona_id: None,
        };

        let error = stage_error(
            CliError::RateLimited,
            &checkpoint,
            "verification_wait",
            Some(serde_json::json!({
                "resumable": false,
                "inspection": {"command": "sunox voice verification-status"}
            })),
        );

        assert_eq!(error.error_code(), "partial_mutation");
        let details = error.details().expect("partial details");
        assert_eq!(details["workflow_id"], "workflow-1");
        assert_eq!(
            details["identifiers"]["sample_processed_id"],
            "processed-main"
        );
        assert_eq!(details["identifiers"]["verification_id"], "verification-1");
        assert_eq!(details["identifiers"]["phrase_id"], "phrase-1");
        assert_eq!(
            details["identifiers"]["verification_processed_id"],
            "processed-verify"
        );
        assert_eq!(details["failed"]["step"], "verification_wait");
        assert_eq!(details["recovery"]["resumable"], false);
        assert_eq!(details["workflow_checkpoint_path"], "/tmp/workflow-1.json");
    }

    #[test]
    fn ambiguous_write_stays_ambiguous_while_gaining_checkpoint_ids() {
        let checkpoint = VoiceCheckpoint {
            workflow_id: "workflow-1".into(),
            phrase_id: "phrase-1".into(),
            completed_steps: vec!["sample_upload_complete"],
            sample_upload_id: Some("upload-main".into()),
            ..VoiceCheckpoint::default()
        };
        let error = stage_error(
            CliError::AmbiguousMutation {
                message: "lost persona response".into(),
                details: serde_json::json!({
                    "operation": "voice_create",
                    "operation_id": "workflow-1",
                    "stage": "persona_create_response_body"
                }),
            },
            &checkpoint,
            "persona_create",
            None,
        );

        assert_eq!(error.error_code(), "ambiguous_mutation");
        let details = error.details().expect("ambiguity details");
        assert_eq!(details["identifiers"]["sample_upload_id"], "upload-main");
        assert_eq!(details["workflow_failed_step"], "persona_create");
        assert_eq!(
            details["workflow_completed_steps"],
            serde_json::json!(["sample_upload_complete"])
        );
    }

    #[tokio::test]
    async fn persona_readback_retries_real_http_404_until_private_vox_is_visible() {
        let (api_url, requests) = mock_status_sequence(vec![
            (404, r#"{"detail":"not found yet"}"#.into()),
            (
                200,
                r#"{"id":"persona-1","name":"My Voice","is_public":false,"persona_type":"vox"}"#
                    .into(),
            ),
        ])
        .await;
        let client = crate::api::SunoClient::new_for_tests(
            api_url,
            AuthState {
                jwt: Some("test-jwt".into()),
                ..AuthState::default()
            },
        )
        .expect("test client");

        let persona = wait_for_private_vox_persona(
            &client,
            "persona-1",
            PollingOptions {
                timeout: Duration::from_secs(2),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect("404 should converge through read-only polling");

        assert_eq!(persona.id, "persona-1");
        assert_eq!(requests.await.expect("requests").len(), 2);
    }

    #[tokio::test]
    async fn verification_poll_rejects_a_mismatched_response_identity() {
        let (api_url, requests) = mock_status_sequence(vec![(
            200,
            r#"{"id":"verification-other","status":"approved"}"#.into(),
        )])
        .await;
        let client = crate::api::SunoClient::new_for_tests(
            api_url,
            AuthState {
                jwt: Some("test-jwt".into()),
                ..AuthState::default()
            },
        )
        .expect("test client");

        let error = wait_for_voice_verification(
            &client,
            VoiceVerification {
                id: "verification-expected".into(),
                status: "pending".into(),
                rejection_reason: None,
                extra: Default::default(),
            },
            PollingOptions {
                timeout: Duration::from_secs(2),
                interval: Duration::from_millis(1),
            },
        )
        .await
        .expect_err("mismatched verification identity must fail closed");

        assert!(matches!(
            error,
            CliError::Api {
                code: "schema_drift",
                ..
            }
        ));
        assert_eq!(requests.await.expect("requests").len(), 1);
    }

    #[tokio::test]
    async fn complete_voice_workflow_uses_two_uploads_and_private_persona_readback() {
        let (transfer_url, transfer_requests) =
            mock_sequence(vec![String::new(), String::new()]).await;
        let api_responses = vec![
            r#"{"credits":1000,"total_credits_left":1000,"monthly_usage":0,"monthly_limit":1000,"is_active":true,"plan":{"id":"tier-pro","name":"Pro","plan_key":"pro","usage_plan_features":[{"name":"persona"}]},"accessible_features":["persona"],"models":[],"period":"monthly","renews_on":null}"#.into(),
            r#"{"phrase_id":"phrase-1","phrase_text":"This is the current phrase"}"#.into(),
            serde_json::json!({
                "id": "upload-main",
                "url": transfer_url.clone(),
                "fields": {}
            })
            .to_string(),
            "{}".into(),
            r#"{"id":"upload-main","status":"complete"}"#.into(),
            r#"{"id":"processed-main","voice_recording_id":"recording-main"}"#.into(),
            r#"{"id":"processed-main","status":"completed"}"#.into(),
            serde_json::json!({
                "id": "upload-verify",
                "url": transfer_url,
                "fields": {}
            })
            .to_string(),
            "{}".into(),
            r#"{"id":"upload-verify","status":"complete"}"#.into(),
            r#"{"id":"processed-verify","voice_recording_id":"recording-verify"}"#.into(),
            r#"{"id":"verification-1","status":"approved"}"#.into(),
            r#"{"id":"persona-1","name":"My Voice","is_public":false,"persona_type":"vox"}"#.into(),
            r#"{"id":"persona-1","name":"My Voice","is_public":true}"#.into(),
            r#"{"id":"persona-1","name":"My Voice","is_public":false,"persona_type":"vox"}"#.into(),
        ];
        let (api_url, api_requests) = mock_sequence(api_responses).await;
        let client = crate::api::SunoClient::new_for_tests(
            api_url,
            AuthState {
                jwt: Some("test-jwt".into()),
                ..AuthState::default()
            },
        )
        .expect("test client");
        let directory = tempfile::tempdir().expect("temp directory");
        let sample = directory.path().join("sample.wav");
        let verification = directory.path().join("verification.wav");
        write_pcm_wav(&sample, 42);
        write_pcm_wav(&verification, 15);

        let result = run(
            &client,
            VoiceCreateInput {
                sample_file: sample,
                verification_file: verification,
                phrase_id: "phrase-1".into(),
                language: "en".into(),
                sample_duration: 42.0,
                name: "My Voice".into(),
                description: None,
                user_input_styles: None,
                singer_skill_level: None,
                confirm_rights: true,
                confirm_eligibility: true,
                confirm_biometric_consent: true,
                checkpoint_dir: Some(directory.path().join("checkpoints")),
                polling: PollingOptions {
                    timeout: Duration::from_secs(2),
                    interval: Duration::from_millis(1),
                },
            },
        )
        .await
        .expect("complete Voice workflow");

        assert_eq!(result.sample_upload_id, "upload-main");
        assert_eq!(result.sample_processed_id, "processed-main");
        assert_eq!(result.phrase_id, "phrase-1");
        assert_eq!(
            result.verification_processed_id.as_deref(),
            Some("processed-verify")
        );
        assert_eq!(result.verification.id, "verification-1");
        assert_eq!(result.persona.id, "persona-1");
        assert!(result.private_readback);
        assert!(result.checkpoint_path.is_file());

        let requests = api_requests.await.expect("API requests");
        assert_eq!(requests.len(), 15);
        assert_eq!(requests[0].path, "/api/billing/info/");
        assert_eq!(
            requests[1].path,
            "/api/voice-verification/phrase/?language=en"
        );
        assert_eq!(requests[2].path, "/api/uploads/audio/");
        assert_eq!(requests[5].path, "/api/processed_clip/voice-vox-stem");
        assert_eq!(requests[10].path, "/api/processed_clip/voice-vox-stem");
        assert_eq!(requests[11].path, "/api/voice-verification/");
        assert_eq!(requests[12].path, "/api/persona/create/");
        assert_eq!(requests[13].path, "/api/persona/get-persona/persona-1/");
        assert_eq!(requests[14].path, "/api/persona/get-persona/persona-1/");
        assert_eq!(requests[2].method, "POST");
        assert_eq!(requests[6].method, "GET");
        let main_process: serde_json::Value =
            serde_json::from_str(&requests[5].body).expect("main process body");
        assert_eq!(main_process["vocal_end_s"], 42.0);
        let persona: serde_json::Value =
            serde_json::from_str(&requests[12].body).expect("persona body");
        assert_eq!(persona["is_public"], false);
        assert_eq!(persona["persona_type"], "vox");
        assert_eq!(persona["verification_id"], "verification-1");

        let transfers = transfer_requests.await.expect("transfer requests");
        assert_eq!(transfers.len(), 2);
        assert!(transfers.iter().all(|request| request.method == "POST"));
    }
}
