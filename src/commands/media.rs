use crate::app::AppContext;
use crate::cli::{DownloadArgs, DownloadFormat, TimedLyricsArgs, UploadArgs, UploadStatusArgs};
use crate::core::{CliError, ensure_clip_ids, ensure_poll_timeout_secs};
use crate::media;
use crate::output::{self, OutputFormat};
use crate::workflow::tasks;
use crate::workflow::upload::{self, UploadWorkflowInput};

#[derive(serde::Serialize)]
struct CompletedDownload {
    clip_id: String,
    path: String,
}

#[derive(Debug, serde::Serialize)]
struct DownloadWarning {
    clip_id: String,
    field: &'static str,
    code: String,
    message: String,
}

struct DownloadFileOptions<'a> {
    output_dir: &'a str,
    video: bool,
    force: bool,
    quiet: bool,
    format: DownloadFormat,
    no_convert: bool,
    skip_timed_lyrics: bool,
}

pub async fn download(args: DownloadArgs, ctx: &AppContext) -> Result<(), CliError> {
    if args.video && args.format.is_some() {
        return Err(CliError::Config(
            "--video cannot be combined with --format".into(),
        ));
    }
    ensure_clip_ids(&args.ids)?;
    let client = ctx.client().await?;
    let clips = tasks::require_found_clips(&args.ids, client.get_clips(&args.ids).await?)?;
    let mut paths = Vec::new();
    let mut completed = Vec::new();
    let mut warnings = Vec::new();
    let output_dir = args.output.as_deref().unwrap_or(&ctx.config.output_dir);
    let format = audio_download_format(args.format);
    let extension = if args.video {
        "mp4"
    } else {
        format.extension()
    };
    preflight_download_batch(&clips, output_dir, extension, args.force).await?;
    for (index, clip) in clips.iter().enumerate() {
        let options = DownloadFileOptions {
            output_dir,
            video: args.video,
            force: args.force,
            quiet: ctx.quiet,
            format,
            no_convert: args.no_convert,
            skip_timed_lyrics: args.skip_timed_lyrics,
        };
        let (path, warning) = match download_file(clip, options, ctx, &client).await {
            Ok(result) => result,
            Err(error) => {
                return Err(partial_download_error(
                    &completed,
                    &clip.id,
                    None,
                    &remaining_clip_ids(&clips[index + 1..]),
                    error,
                ));
            }
        };

        if let Some(warning) = warning {
            if !ctx.quiet {
                eprintln!("Warning: {}", warning.message);
            }
            warnings.push(warning);
        }

        if !ctx.quiet {
            eprintln!("Downloaded: {path}");
        }
        completed.push(CompletedDownload {
            clip_id: clip.id.clone(),
            path: path.clone(),
        });
        paths.push(path);
    }
    match ctx.fmt {
        OutputFormat::Json if warnings.is_empty() => output::json::success(&paths),
        OutputFormat::Json => output::json::success_with_warnings(&paths, &warnings),
        OutputFormat::Table => {}
    }
    Ok(())
}

async fn preflight_download_batch(
    clips: &[crate::api::types::Clip],
    output_dir: &str,
    extension: &str,
    force: bool,
) -> Result<(), CliError> {
    for clip in clips {
        media::download::preflight_clip_download(clip, output_dir, extension, force).await?;
    }
    Ok(())
}

async fn download_file(
    clip: &crate::api::types::Clip,
    options: DownloadFileOptions<'_>,
    ctx: &AppContext,
    client: &crate::api::SunoClient,
) -> Result<(String, Option<DownloadWarning>), CliError> {
    if options.video {
        return media::download_clip(clip, options.output_dir, true, options.force, options.quiet)
            .await
            .map(|path| (path, None));
    }
    media::download::preflight_clip_download(
        clip,
        options.output_dir,
        options.format.extension(),
        options.force,
    )
    .await?;
    let url =
        official_download_url(ctx, client, &clip.id, options.format, options.no_convert).await?;
    if should_fetch_timed_lyrics(options.format, options.skip_timed_lyrics) {
        download_mp3_with_lyrics(
            clip,
            options.output_dir,
            &url,
            options.force,
            options.quiet,
            ctx,
            client,
        )
        .await
    } else {
        media::download_clip_url(
            clip,
            options.output_dir,
            &url,
            options.format.extension(),
            options.force,
            options.quiet,
        )
        .await
        .map(|path| (path, None))
    }
}

fn should_fetch_timed_lyrics(format: DownloadFormat, skip_timed_lyrics: bool) -> bool {
    format == DownloadFormat::Mp3 && !skip_timed_lyrics
}

async fn download_mp3_with_lyrics(
    clip: &crate::api::types::Clip,
    output_dir: &str,
    url: &str,
    force: bool,
    quiet: bool,
    ctx: &AppContext,
    client: &crate::api::SunoClient,
) -> Result<(String, Option<DownloadWarning>), CliError> {
    let staged = media::stage_clip_url(clip, output_dir, url, "mp3", force, quiet).await?;
    let plain_lyrics = clip_alignment_lyrics(clip);
    let aligned_result = if ctx.read_only {
        client
            .existing_aligned_lyrics(&clip.id, configured_polling(ctx))
            .await
    } else {
        let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
        client
            .aligned_lyrics(
                &clip.id,
                plain_lyrics,
                !clip_has_concat_history(clip),
                configured_polling(ctx),
            )
            .await
    };
    let (aligned, warning) = match aligned_result {
        Ok(aligned) => (Some(aligned), None),
        Err(error) if error.is_auth_or_rate_limit() => return Err(error),
        Err(error) => {
            let warning = DownloadWarning {
                clip_id: clip.id.clone(),
                field: "aligned_lyrics",
                code: error.error_code().to_string(),
                message: format!(
                    "downloaded {} but timed lyrics could not be embedded: {error}",
                    clip.id
                ),
            };
            (None, Some(warning))
        }
    };
    let path = staged.commit_after(|temporary_path| {
        media::embed_lyrics_in_mp3(
            &temporary_path.to_string_lossy(),
            &clip.title,
            plain_lyrics,
            aligned.as_deref(),
        )
    })?;
    if !quiet {
        if aligned.is_some() {
            eprintln!("Embedded plain and timed lyrics into {path}");
        } else {
            eprintln!("Embedded available plain lyrics into {path}");
        }
    }
    Ok((path, warning))
}

fn remaining_clip_ids(clips: &[crate::api::types::Clip]) -> Vec<String> {
    clips.iter().map(|clip| clip.id.clone()).collect()
}

fn partial_download_error(
    succeeded: &[CompletedDownload],
    failed_clip_id: &str,
    partial_output_path: Option<&str>,
    not_attempted_clip_ids: &[String],
    error: CliError,
) -> CliError {
    if succeeded.is_empty() && partial_output_path.is_none() {
        return error;
    }

    let mut failed = serde_json::json!({
        "clip_id": failed_clip_id,
        "code": error.error_code(),
        "message": error.to_string(),
    });
    if let Some(details) = error.details() {
        failed["details"] = details.clone();
    }
    if let Some(path) = partial_output_path {
        failed["output_path"] = serde_json::Value::String(path.to_string());
    }
    CliError::PartialDownload {
        message: format!(
            "download completed for {} clip(s), failed for {}, and left {} clip(s) not attempted",
            succeeded.len(),
            failed_clip_id,
            not_attempted_clip_ids.len()
        ),
        details: serde_json::json!({
            "succeeded": succeeded,
            "failed": failed,
            "not_attempted_clip_ids": not_attempted_clip_ids,
        }),
    }
}

fn audio_download_format(format: Option<DownloadFormat>) -> DownloadFormat {
    format.unwrap_or(DownloadFormat::Mp3)
}

async fn official_download_url(
    ctx: &AppContext,
    client: &crate::api::SunoClient,
    clip_id: &str,
    format: DownloadFormat,
    no_convert: bool,
) -> Result<String, CliError> {
    let polling = configured_polling(ctx);
    let allow_conversion = !no_convert && !ctx.read_only;
    if format.requires_mutation_lock() && allow_conversion {
        let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
        client.download_url(clip_id, format, polling).await
    } else if allow_conversion {
        client.download_url(clip_id, format, polling).await
    } else {
        client
            .download_url_with_conversion_policy(clip_id, format, polling, false)
            .await
    }
}

fn configured_polling(ctx: &AppContext) -> crate::api::PollingOptions {
    crate::api::PollingOptions {
        timeout: std::time::Duration::from_secs(ctx.config.poll_timeout_secs),
        interval: std::time::Duration::from_secs(ctx.config.poll_interval_secs.max(1)),
    }
}

pub async fn upload(args: UploadArgs, ctx: &AppContext) -> Result<(), CliError> {
    let timeout_secs = args.timeout.unwrap_or(ctx.config.poll_timeout_secs);
    ensure_poll_timeout_secs(timeout_secs)?;
    let lyrics = match (&args.lyrics, &args.lyrics_file) {
        (Some(lyrics), _) => Some(lyrics.clone()),
        (_, Some(path)) => Some(std::fs::read_to_string(path)?),
        _ => None,
    };
    let path = std::path::Path::new(&args.file);
    if !ctx.quiet {
        eprintln!("Uploading audio: {}", path.display());
    }

    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = upload::run(
        &client,
        UploadWorkflowInput {
            file: path,
            upload_type: &args.upload_type,
            is_stem_mix: args.stem_mix,
            title: args.title,
            lyrics,
            timeout: std::time::Duration::from_secs(timeout_secs),
            poll_interval: std::time::Duration::from_secs(ctx.config.poll_interval_secs),
        },
    )
    .await?;

    match ctx.fmt {
        OutputFormat::Json => output::json::success(&result),
        OutputFormat::Table => {
            eprintln!("Upload complete: {}", result.upload_id);
            if let Some(clip_id) = result.clip_id {
                println!("{clip_id}");
            }
        }
    }
    Ok(())
}

pub async fn upload_status(args: UploadStatusArgs, ctx: &AppContext) -> Result<(), CliError> {
    let status = ctx
        .client()
        .await?
        .get_audio_upload(&args.upload_id)
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&status),
        OutputFormat::Table => {
            println!(
                "Upload: {}",
                status.id.as_deref().unwrap_or(&args.upload_id)
            );
            println!("Status: {}", status.status.as_deref().unwrap_or("unknown"));
            if let Some(title) = status.title {
                println!("Title: {title}");
            }
            if let Some(has_vocal) = status.has_vocal {
                println!("Has vocal: {has_vocal}");
            }
        }
    }
    Ok(())
}

pub async fn timed_lyrics(args: TimedLyricsArgs, ctx: &AppContext) -> Result<(), CliError> {
    let render = timed_lyrics_render(args.lrc, ctx.fmt, ctx.json_explicit)?;
    let client = ctx.client().await?;
    let ids = vec![args.id.clone()];
    let clips = tasks::require_found_clips(&ids, client.get_clips(&ids).await?)?;
    let clip = &clips[0];
    let words = if ctx.read_only {
        client
            .existing_aligned_lyrics(&args.id, configured_polling(ctx))
            .await?
    } else {
        let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
        client
            .aligned_lyrics(
                &args.id,
                clip_alignment_lyrics(clip),
                !clip_has_concat_history(clip),
                configured_polling(ctx),
            )
            .await?
    };
    match render {
        TimedLyricsRender::Json => output::json::success(&words),
        TimedLyricsRender::Lrc => {
            for word in &words {
                if !word.success {
                    continue;
                }
                let mins = (word.start_s / 60.0) as u32;
                let secs = word.start_s % 60.0;
                println!("[{:02}:{:05.2}] {}", mins, secs, word.word);
            }
        }
        TimedLyricsRender::Table => {
            for word in &words {
                if word.success {
                    println!(
                        "{:>6.2}s - {:>6.2}s  {}",
                        word.start_s, word.end_s, word.word
                    );
                }
            }
        }
    }
    Ok(())
}

fn clip_alignment_lyrics(clip: &crate::api::types::Clip) -> Option<&str> {
    clip.metadata
        .extra
        .get("infill_lyrics")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            clip.metadata
                .prompt
                .as_deref()
                .filter(|value| !value.is_empty())
        })
}

fn clip_has_concat_history(clip: &crate::api::types::Clip) -> bool {
    clip.metadata
        .extra
        .get("concat_history")
        .is_some_and(json_value_is_truthy)
}

fn json_value_is_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
        serde_json::Value::String(value) => !value.is_empty(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => true,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TimedLyricsRender {
    Json,
    Lrc,
    Table,
}

fn timed_lyrics_render(
    lrc: bool,
    fmt: OutputFormat,
    json_explicit: bool,
) -> Result<TimedLyricsRender, CliError> {
    if lrc && json_explicit {
        return Err(CliError::Config(
            "--lrc cannot be combined with explicit --json".into(),
        ));
    }
    Ok(if lrc {
        TimedLyricsRender::Lrc
    } else if matches!(fmt, OutputFormat::Json) {
        TimedLyricsRender::Json
    } else {
        TimedLyricsRender::Table
    })
}

#[cfg(test)]
mod tests {
    use crate::cli::DownloadFormat;
    use crate::core::CliError;
    use crate::output::OutputFormat;

    use super::{
        DownloadFileOptions, TimedLyricsRender, audio_download_format, clip_alignment_lyrics,
        download_file, json_value_is_truthy, partial_download_error, preflight_download_batch,
        should_fetch_timed_lyrics, timed_lyrics_render,
    };

    fn clip() -> crate::api::types::Clip {
        serde_json::from_value(serde_json::json!({
            "id": "clip-a",
            "title": "Track",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-08-24T00:00:00Z"
        }))
        .expect("clip fixture")
    }

    fn clip_with_id(id: &str) -> crate::api::types::Clip {
        let mut clip = clip();
        clip.id = id.to_owned();
        clip
    }

    fn context() -> crate::app::AppContext {
        crate::app::AppContext {
            fmt: OutputFormat::Json,
            json_explicit: true,
            quiet: true,
            parallel: true,
            read_only: false,
            config: crate::core::AppConfig::default(),
        }
    }

    #[test]
    fn default_audio_download_uses_official_mp3_route() {
        assert_eq!(audio_download_format(None), DownloadFormat::Mp3);
    }

    #[test]
    fn explicit_audio_format_uses_official_download_route() {
        assert_eq!(
            audio_download_format(Some(DownloadFormat::Wav)),
            DownloadFormat::Wav
        );
    }

    #[test]
    fn conversion_formats_require_the_account_mutation_lock() {
        assert!(!DownloadFormat::Mp3.requires_mutation_lock());
        assert!(!DownloadFormat::M4a.requires_mutation_lock());
        assert!(DownloadFormat::Wav.requires_mutation_lock());
        assert!(DownloadFormat::Opus.requires_mutation_lock());
    }

    #[test]
    fn stem_download_safety_switch_prevents_aligned_lyrics_generation() {
        assert!(should_fetch_timed_lyrics(DownloadFormat::Mp3, false));
        assert!(!should_fetch_timed_lyrics(DownloadFormat::Mp3, true));
        assert!(!should_fetch_timed_lyrics(DownloadFormat::Wav, true));
    }

    #[tokio::test]
    async fn existing_audio_destinations_fail_before_prepared_or_conversion_endpoints() {
        let dir = tempfile::tempdir().expect("download output directory");
        let output_dir = dir.path().to_string_lossy().into_owned();
        let client = crate::api::SunoClient::new_for_tests(
            "http://127.0.0.1:9".into(),
            crate::auth::AuthState {
                jwt: Some("test-jwt".into()),
                ..Default::default()
            },
        )
        .expect("test client");

        for format in [DownloadFormat::Mp3, DownloadFormat::Wav] {
            let destination = dir
                .path()
                .join(format!("track-clip-a.{}", format.extension()));
            std::fs::write(&destination, b"existing").expect("existing destination");

            let result = download_file(
                &clip(),
                DownloadFileOptions {
                    output_dir: &output_dir,
                    video: false,
                    force: false,
                    quiet: true,
                    format,
                    no_convert: false,
                    skip_timed_lyrics: false,
                },
                &context(),
                &client,
            )
            .await;
            let error = match result {
                Ok(_) => panic!("local destination must fail before any official endpoint call"),
                Err(error) => error,
            };

            assert!(
                matches!(error, CliError::Download(message) if message.contains("already exists")),
                "format {format:?} reached the network before local preflight"
            );
        }
    }

    #[tokio::test]
    async fn batch_preflight_rejects_a_later_collision_before_downloads_start() {
        let dir = tempfile::tempdir().expect("download output directory");
        let output_dir = dir.path().to_string_lossy().into_owned();
        std::fs::write(dir.path().join("track-clip-b.mp3"), b"existing")
            .expect("existing second destination");

        let error = preflight_download_batch(
            &[clip_with_id("clip-a"), clip_with_id("clip-b")],
            &output_dir,
            "mp3",
            false,
        )
        .await
        .expect_err("the complete batch must be preflighted before requests start");

        assert!(matches!(error, CliError::Download(message) if message.contains("already exists")));
        assert!(!dir.path().join("track-clip-a.mp3").exists());
    }

    #[test]
    fn partial_download_reports_completed_paths_and_remaining_ids() {
        let error = partial_download_error(
            &[super::CompletedDownload {
                clip_id: "clip-complete".into(),
                path: "/tmp/complete.mp3".into(),
            }],
            "clip-failed",
            None,
            &[("clip-later".to_string())],
            CliError::Download("network dropped".into()),
        );

        assert_eq!(error.error_code(), "partial_download");
        assert_eq!(
            error.details().expect("partial download details")["succeeded"][0]["clip_id"],
            "clip-complete"
        );
        assert_eq!(
            error.details().expect("partial download details")["failed"]["clip_id"],
            "clip-failed"
        );
        assert_eq!(
            error.details().expect("partial download details")["not_attempted_clip_ids"],
            serde_json::json!(["clip-later"])
        );
    }

    #[test]
    fn partial_download_preserves_nested_ambiguous_recovery_details() {
        let error = partial_download_error(
            &[super::CompletedDownload {
                clip_id: "clip-complete".into(),
                path: "/tmp/complete.mp3".into(),
            }],
            "clip-failed",
            None,
            &[],
            CliError::AmbiguousMutation {
                message: "conversion outcome is unknown".into(),
                details: serde_json::json!({
                    "operation_id": "conversion-1",
                    "recovery": {
                        "resumable": true,
                        "inspection_commands": ["sunox clip download clip-failed --format wav --no-convert --json"]
                    }
                }),
            },
        );

        let failed = &error.details().expect("partial download details")["failed"];
        assert_eq!(failed["code"], "ambiguous_mutation");
        assert_eq!(failed["details"]["operation_id"], "conversion-1");
        assert_eq!(failed["details"]["recovery"]["resumable"], true);
        assert_eq!(
            failed["details"]["recovery"]["inspection_commands"][0],
            "sunox clip download clip-failed --format wav --no-convert --json"
        );
    }

    #[test]
    fn timed_lyrics_lrc_overrides_auto_detected_json_output() {
        assert_eq!(
            timed_lyrics_render(true, OutputFormat::Json, false)
                .expect("auto JSON may yield to LRC"),
            TimedLyricsRender::Lrc
        );
    }

    #[test]
    fn timed_lyrics_rejects_explicit_json_with_lrc() {
        let error = timed_lyrics_render(true, OutputFormat::Json, true)
            .expect_err("explicit formats conflict");

        assert_eq!(error.error_code(), "config_error");
    }

    #[test]
    fn timed_lyrics_lrc_applies_to_table_output() {
        assert_eq!(
            timed_lyrics_render(true, OutputFormat::Table, false).expect("LRC"),
            TimedLyricsRender::Lrc
        );
    }

    #[test]
    fn timed_lyrics_table_output_is_default_human_format() {
        assert_eq!(
            timed_lyrics_render(false, OutputFormat::Table, false).expect("table"),
            TimedLyricsRender::Table
        );
    }

    #[test]
    fn concat_history_uses_javascript_truthiness_for_augmentation() {
        assert!(json_value_is_truthy(&serde_json::json!([])));
        assert!(json_value_is_truthy(&serde_json::json!({})));
        assert!(json_value_is_truthy(&serde_json::json!("history")));
        assert!(!json_value_is_truthy(&serde_json::Value::Null));
        assert!(!json_value_is_truthy(&serde_json::json!(false)));
        assert!(!json_value_is_truthy(&serde_json::json!(0)));
        assert!(!json_value_is_truthy(&serde_json::json!("")));
    }

    #[test]
    fn empty_infill_lyrics_fall_back_to_the_clip_prompt() {
        let clip: crate::api::types::Clip = serde_json::from_value(serde_json::json!({
            "id": "clip-1",
            "title": "Song",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-07-19T00:00:00Z",
            "metadata": {
                "infill_lyrics": "",
                "prompt": "[Verse]\nWords"
            }
        }))
        .expect("clip");

        assert_eq!(clip_alignment_lyrics(&clip), Some("[Verse]\nWords"));
    }
}
