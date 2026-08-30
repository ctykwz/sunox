use std::collections::{HashMap, HashSet};

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

#[derive(Clone, Debug, serde::Serialize)]
struct AuthorizedDownloadSource {
    source_clip_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    credit_deducted: Option<bool>,
}

#[derive(Clone, Debug)]
struct DownloadAccess {
    source_clip_id: String,
    authorized_now: bool,
    credit_deducted: Option<bool>,
}

#[derive(Clone, Debug)]
enum DownloadSource {
    EachTarget,
    SharedParent(String),
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
    download_with_source(args, DownloadSource::EachTarget, ctx).await
}

pub(super) async fn download_with_shared_source(
    args: DownloadArgs,
    source_clip_id: String,
    ctx: &AppContext,
) -> Result<(), CliError> {
    download_with_source(args, DownloadSource::SharedParent(source_clip_id), ctx).await
}

async fn download_with_source(
    args: DownloadArgs,
    source: DownloadSource,
    ctx: &AppContext,
) -> Result<(), CliError> {
    if args.video && args.format.is_some() {
        return Err(CliError::Config(
            "--video cannot be combined with --format".into(),
        ));
    }
    ensure_clip_ids(&args.ids)?;
    let ids = deduplicate_clip_ids(&args.ids);
    let client = ctx.client().await?;
    let clips = tasks::require_found_clips(&ids, client.get_clips(&ids).await?)?;
    let mut paths = Vec::new();
    let mut completed = Vec::new();
    let mut warnings = Vec::new();
    let mut access_by_source = HashMap::new();
    let mut authorized_sources = Vec::new();
    let output_dir = args.output.as_deref().unwrap_or(&ctx.config.output_dir);
    let format = audio_download_format(args.format);
    let extension = if args.video {
        "mp4"
    } else {
        format.extension()
    };
    preflight_download_batch(&clips, output_dir, extension, args.force).await?;
    for (index, clip) in clips.iter().enumerate() {
        let source_clip_id = download_source_id(&source, clip).to_owned();
        let hinted_unlock = match &source {
            DownloadSource::EachTarget => clip.is_download_unlocked,
            DownloadSource::SharedParent(_) => None,
        };
        let (access, warning, first_source_use) = match cached_download_access(
            &mut access_by_source,
            &source_clip_id,
            hinted_unlock,
            ctx,
            &client,
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                return Err(partial_download_error(
                    &completed,
                    &clip.id,
                    None,
                    &remaining_clip_ids(&clips[index + 1..]),
                    &authorized_sources,
                    &warnings,
                    error,
                ));
            }
        };
        if first_source_use && access.authorized_now {
            authorized_sources.push(AuthorizedDownloadSource {
                source_clip_id: access.source_clip_id.clone(),
                credit_deducted: access.credit_deducted,
            });
        }
        if let Some(warning) = warning {
            if !ctx.quiet {
                eprintln!("Warning: {}", warning.message);
            }
            warnings.push(warning);
        }
        let options = DownloadFileOptions {
            output_dir,
            video: args.video,
            force: args.force,
            quiet: ctx.quiet,
            format,
            no_convert: args.no_convert,
            skip_timed_lyrics: args.skip_timed_lyrics,
        };
        let (path, warning) = match download_file(clip, options, &access, ctx, &client).await {
            Ok(result) => result,
            Err(error) => {
                return Err(partial_download_error(
                    &completed,
                    &clip.id,
                    None,
                    &remaining_clip_ids(&clips[index + 1..]),
                    &authorized_sources,
                    &warnings,
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

async fn cached_download_access(
    cache: &mut HashMap<String, DownloadAccess>,
    source_clip_id: &str,
    hinted_unlock: Option<bool>,
    ctx: &AppContext,
    client: &crate::api::SunoClient,
) -> Result<(DownloadAccess, Option<DownloadWarning>, bool), CliError> {
    if let Some(access) = cache.get(source_clip_id) {
        return Ok((access.clone(), None, false));
    }
    let (access, warning) =
        ensure_download_access(source_clip_id, hinted_unlock, ctx, client).await?;
    cache.insert(source_clip_id.to_owned(), access.clone());
    Ok((access, warning, true))
}

fn deduplicate_clip_ids(ids: &[String]) -> Vec<String> {
    let mut seen = HashSet::with_capacity(ids.len());
    ids.iter()
        .filter(|id| seen.insert((*id).clone()))
        .cloned()
        .collect()
}

fn download_source_id<'a>(
    source: &'a DownloadSource,
    target: &'a crate::api::types::Clip,
) -> &'a str {
    match source {
        DownloadSource::EachTarget => &target.id,
        DownloadSource::SharedParent(source_clip_id) => source_clip_id,
    }
}

fn read_only_download_authorization_error(
    source_clip_id: &str,
    is_download_unlocked: Option<bool>,
) -> CliError {
    CliError::Diagnostic {
        code: "download_authorization_required",
        message: format!(
            "clip {source_clip_id} is not proven download-unlocked; --read-only refused Suno's authorization write"
        ),
        details: serde_json::json!({
            "source_clip_id": source_clip_id,
            "is_download_unlocked": is_download_unlocked,
            "authorization_post_sent": false,
            "download_started": false,
            "recovery": "inspect the clip and live download usage, then rerun without --read-only only when consuming download allowance is intentional",
            "inspection_commands": [
                format!("sunox clip info {source_clip_id} --json"),
                "sunox credits --json"
            ]
        }),
    }
}

async fn ensure_download_access(
    source_clip_id: &str,
    hinted_unlock: Option<bool>,
    ctx: &AppContext,
    client: &crate::api::SunoClient,
) -> Result<(DownloadAccess, Option<DownloadWarning>), CliError> {
    if hinted_unlock == Some(true) {
        return Ok((existing_download_access(source_clip_id), None));
    }

    let observed_unlock = exact_download_unlock_state(client, source_clip_id).await?;
    if observed_unlock == Some(true) {
        return Ok((existing_download_access(source_clip_id), None));
    }
    if ctx.read_only {
        return Err(read_only_download_authorization_error(
            source_clip_id,
            observed_unlock,
        ));
    }

    let mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
    let locked_unlock = exact_download_unlock_state(client, source_clip_id).await?;
    if locked_unlock == Some(true) {
        drop(mutation_guard);
        return Ok((existing_download_access(source_clip_id), None));
    }

    let authorization = match client.authorize_download(source_clip_id).await {
        Ok(authorization) => authorization,
        Err(error) if matches!(error, CliError::AmbiguousMutation { .. }) => {
            let reconciled = reconcile_download_authorization(source_clip_id, client, error).await;
            drop(mutation_guard);
            return reconciled;
        }
        Err(error) => {
            drop(mutation_guard);
            return Err(error);
        }
    };
    drop(mutation_guard);

    if authorization.ok != Some(true) {
        return Err(download_authorization_denied(
            source_clip_id,
            &authorization,
        ));
    }

    let warning = if authorization.credit_deducted == Some(true) {
        billing_readback_warning(source_clip_id, client.billing_info().await.err())
    } else {
        None
    };
    Ok((
        DownloadAccess {
            source_clip_id: source_clip_id.to_owned(),
            authorized_now: true,
            credit_deducted: authorization.credit_deducted,
        },
        warning,
    ))
}

fn existing_download_access(source_clip_id: &str) -> DownloadAccess {
    DownloadAccess {
        source_clip_id: source_clip_id.to_owned(),
        authorized_now: false,
        credit_deducted: None,
    }
}

async fn exact_download_unlock_state(
    client: &crate::api::SunoClient,
    source_clip_id: &str,
) -> Result<Option<bool>, CliError> {
    let clip = client
        .get_clip(source_clip_id)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("download source clip {source_clip_id}")))?;
    if clip.id != source_clip_id {
        return Err(CliError::Api {
            code: "schema_drift",
            message: format!(
                "download source readback returned clip {} while {source_clip_id} was requested",
                clip.id
            ),
        });
    }
    Ok(clip.is_download_unlocked)
}

async fn reconcile_download_authorization(
    source_clip_id: &str,
    client: &crate::api::SunoClient,
    error: CliError,
) -> Result<(DownloadAccess, Option<DownloadWarning>), CliError> {
    let clip_result = exact_download_unlock_state(client, source_clip_id).await;
    let billing_result = client.billing_info().await;
    if matches!(&clip_result, Ok(Some(true))) {
        return Ok((
            DownloadAccess {
                source_clip_id: source_clip_id.to_owned(),
                authorized_now: true,
                credit_deducted: None,
            },
            billing_readback_warning(source_clip_id, billing_result.err()),
        ));
    }
    Err(attach_download_authorization_readback(
        error,
        clip_result,
        billing_result,
    ))
}

fn attach_download_authorization_readback(
    error: CliError,
    clip_result: Result<Option<bool>, CliError>,
    billing_result: Result<crate::api::types::BillingInfo, CliError>,
) -> CliError {
    let CliError::AmbiguousMutation {
        message,
        mut details,
    } = error
    else {
        return error;
    };
    let Some(fields) = details.as_object_mut() else {
        return CliError::AmbiguousMutation { message, details };
    };
    fields.insert(
        "readback".into(),
        serde_json::json!({
            "is_download_unlocked": result_evidence(clip_result),
            "billing": billing_readback_evidence(billing_result),
        }),
    );
    CliError::AmbiguousMutation { message, details }
}

fn result_evidence(result: Result<Option<bool>, CliError>) -> serde_json::Value {
    match result {
        Ok(value) => serde_json::json!({"ok": true, "value": value}),
        Err(error) => serde_json::json!({
            "ok": false,
            "code": error.error_code(),
            "message": error.to_string(),
        }),
    }
}

fn billing_readback_evidence(
    result: Result<crate::api::types::BillingInfo, CliError>,
) -> serde_json::Value {
    match result {
        Ok(info) => serde_json::json!({
            "ok": true,
            "download_usage": super::account::safe_download_usage_value(
                info.download_usage.as_ref()
            ),
        }),
        Err(error) => serde_json::json!({
            "ok": false,
            "code": error.error_code(),
            "message": error.to_string(),
        }),
    }
}

fn billing_readback_warning(
    source_clip_id: &str,
    error: Option<CliError>,
) -> Option<DownloadWarning> {
    error.map(|error| DownloadWarning {
        clip_id: source_clip_id.to_owned(),
        field: "download_usage",
        code: error.error_code().to_owned(),
        message: format!(
            "download authorization for {source_clip_id} succeeded, but current download usage could not be refreshed: {error}"
        ),
    })
}

fn download_authorization_denied(
    source_clip_id: &str,
    authorization: &crate::api::types::DownloadAuthorizationResponse,
) -> CliError {
    CliError::Diagnostic {
        code: "download_authorization_denied",
        message: authorization.message.clone().unwrap_or_else(|| {
            format!("Suno did not authorize a download for clip {source_clip_id}")
        }),
        details: serde_json::json!({
            "source_clip_id": source_clip_id,
            "ok": authorization.ok,
            "reason": authorization.reason.as_deref(),
            "message": authorization.message.as_deref(),
            "credit_deducted": authorization.credit_deducted,
            "authorization_post_sent": true,
            "download_started": false,
            "inspection_commands": [
                format!("sunox clip info {source_clip_id} --json"),
                "sunox credits --json"
            ]
        }),
    }
}

async fn preflight_download_batch(
    clips: &[crate::api::types::Clip],
    output_dir: &str,
    extension: &str,
    force: bool,
) -> Result<(), CliError> {
    let mut planned_destinations = HashMap::new();
    for clip in clips {
        let destination = media::download::planned_clip_download_path(clip, output_dir, extension);
        if let Some(first_clip_id) = planned_destinations.insert(destination.clone(), &clip.id) {
            return Err(CliError::Diagnostic {
                code: "download_destination_collision",
                message: format!(
                    "clips {first_clip_id} and {} resolve to the same output file: {}",
                    clip.id,
                    destination.display()
                ),
                details: serde_json::json!({
                    "first_clip_id": first_clip_id,
                    "second_clip_id": clip.id,
                    "destination": destination,
                    "extension": extension,
                    "authorization_post_sent": false,
                    "download_started": false,
                }),
            });
        }
    }
    for clip in clips {
        media::download::preflight_clip_download(clip, output_dir, extension, force).await?;
    }
    Ok(())
}

async fn download_file(
    clip: &crate::api::types::Clip,
    options: DownloadFileOptions<'_>,
    access: &DownloadAccess,
    ctx: &AppContext,
    client: &crate::api::SunoClient,
) -> Result<(String, Option<DownloadWarning>), CliError> {
    debug_assert!(!access.source_clip_id.is_empty());
    if options.video {
        let url = video_download_url(client, clip, access, configured_polling(ctx)).await?;
        return media::download_clip_url(
            clip,
            options.output_dir,
            &url,
            "mp4",
            options.force,
            options.quiet,
        )
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
    authorized_sources: &[AuthorizedDownloadSource],
    warnings: &[DownloadWarning],
    error: CliError,
) -> CliError {
    if succeeded.is_empty() && partial_output_path.is_none() && authorized_sources.is_empty() {
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
            "authorized_sources": authorized_sources,
            "warnings": warnings,
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
    let prepared_format = match format {
        DownloadFormat::Mp3 => Some(crate::api::download::PreparedDownloadFormat::Mp3),
        DownloadFormat::M4a => Some(crate::api::download::PreparedDownloadFormat::M4a),
        DownloadFormat::Wav => Some(crate::api::download::PreparedDownloadFormat::Wav),
        DownloadFormat::Opus => None,
    };
    if let Some(prepared_format) = prepared_format {
        match client
            .prepared_download_url(clip_id, prepared_format, polling)
            .await
        {
            Ok(url) => return Ok(url),
            Err(error)
                if format == DownloadFormat::Wav && prepared_download_is_unavailable(&error) => {}
            Err(error) => return Err(error),
        }
    }

    legacy_download_url(ctx, client, clip_id, format, polling, no_convert).await
}

async fn legacy_download_url(
    ctx: &AppContext,
    client: &crate::api::SunoClient,
    clip_id: &str,
    format: DownloadFormat,
    polling: crate::api::PollingOptions,
    no_convert: bool,
) -> Result<String, CliError> {
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

async fn video_download_url(
    client: &crate::api::SunoClient,
    clip: &crate::api::types::Clip,
    access: &DownloadAccess,
    polling: crate::api::PollingOptions,
) -> Result<String, CliError> {
    debug_assert!(!access.source_clip_id.is_empty());
    match client
        .prepared_download_url(
            &clip.id,
            crate::api::download::PreparedDownloadFormat::Mp4,
            polling,
        )
        .await
    {
        Ok(url) => Ok(url),
        Err(error) if prepared_download_is_unavailable(&error) => clip
            .video_url
            .as_deref()
            .filter(|url| usable_legacy_video_url(url))
            .map(str::to_owned)
            .ok_or_else(|| {
                CliError::Download(format!(
                    "no prepared or usable legacy MP4 URL is available for clip {}",
                    clip.id
                ))
            }),
        Err(error) => Err(error),
    }
}

fn prepared_download_is_unavailable(error: &CliError) -> bool {
    matches!(error, CliError::SunoApi { status: 404, .. })
        || error.error_code() == "prepared_download_unavailable"
}

fn usable_legacy_video_url(url: &str) -> bool {
    let normalized = url.trim().split(['?', '#']).next().unwrap_or_default();
    !normalized.is_empty()
        && normalized.trim_end_matches('/') != "/api/forbidden"
        && !normalized.trim_end_matches('/').ends_with("/api/forbidden")
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
    use std::collections::HashMap;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use crate::cli::DownloadFormat;
    use crate::core::CliError;
    use crate::output::OutputFormat;

    use super::{
        AuthorizedDownloadSource, DownloadAccess, DownloadFileOptions, DownloadSource,
        DownloadWarning, TimedLyricsRender, audio_download_format, cached_download_access,
        clip_alignment_lyrics, deduplicate_clip_ids, download_file, download_source_id,
        ensure_download_access, json_value_is_truthy, official_download_url,
        partial_download_error, preflight_download_batch, prepared_download_is_unavailable,
        read_only_download_authorization_error, should_fetch_timed_lyrics, timed_lyrics_render,
        usable_legacy_video_url, video_download_url,
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
        context_with_read_only(false)
    }

    fn context_with_read_only(read_only: bool) -> crate::app::AppContext {
        crate::app::AppContext {
            fmt: OutputFormat::Json,
            json_explicit: true,
            quiet: true,
            parallel: true,
            read_only,
            config: crate::core::AppConfig::default(),
        }
    }

    #[derive(Debug)]
    struct TestRequest {
        method: String,
        path: String,
    }

    struct SequenceServer {
        base_url: String,
        requests: oneshot::Receiver<Vec<TestRequest>>,
    }

    impl SequenceServer {
        async fn start(responses: Vec<(u16, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind download command test server");
            let address = listener.local_addr().expect("download server address");
            let (sender, requests) = oneshot::channel();
            tokio::spawn(async move {
                let mut captured = Vec::new();
                for (status, body) in responses {
                    let (mut stream, _) = listener.accept().await.expect("test request");
                    let mut bytes = Vec::new();
                    let mut chunk = [0_u8; 2048];
                    loop {
                        let read = stream.read(&mut chunk).await.expect("read test request");
                        if read == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&chunk[..read]);
                        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let head = String::from_utf8_lossy(&bytes);
                    let request_line = head.lines().next().expect("HTTP request line");
                    let mut parts = request_line.split_whitespace();
                    captured.push(TestRequest {
                        method: parts.next().expect("request method").to_owned(),
                        path: parts.next().expect("request path").to_owned(),
                    });
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream
                        .write_all(response.as_bytes())
                        .await
                        .expect("write test response");
                }
                let _ = sender.send(captured);
            });
            Self {
                base_url: format!("http://{address}"),
                requests,
            }
        }

        fn client(&self) -> crate::api::SunoClient {
            crate::api::SunoClient::new_for_tests(
                self.base_url.clone(),
                crate::auth::AuthState {
                    jwt: Some("test-jwt".into()),
                    ..Default::default()
                },
            )
            .expect("download test client")
        }

        async fn captured(self) -> Vec<TestRequest> {
            self.requests.await.expect("captured download requests")
        }
    }

    fn source_clip_json(unlocked: Option<bool>) -> String {
        let mut clip = serde_json::json!({
            "id": "source-1",
            "title": "Source",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-08-30T00:00:00Z"
        });
        if let Some(unlocked) = unlocked {
            clip["is_download_unlocked"] = serde_json::json!(unlocked);
        }
        clip.to_string()
    }

    fn billing_json() -> String {
        serde_json::json!({
            "credits": 1,
            "total_credits_left": 1,
            "monthly_usage": 0,
            "monthly_limit": 1,
            "is_active": true,
            "plan": {"name": "Pro", "plan_key": "pro", "usage_plan_features": []},
            "models": [],
            "period": "monthly",
            "renews_on": null,
            "remaster_model_types": [],
            "download_usage": {
                "current_period_downloads_limit": 20,
                "current_period_downloads_used": 1,
                "additional_download_remaining": 0,
                "session_token": "must-not-leak-billing-readback"
            }
        })
        .to_string()
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

    #[test]
    fn prepared_fallback_only_accepts_explicit_unavailable_or_not_found() {
        assert!(prepared_download_is_unavailable(&CliError::Diagnostic {
            code: "prepared_download_unavailable",
            message: "not prepared".into(),
            details: serde_json::json!({}),
        }));
        assert!(prepared_download_is_unavailable(&CliError::SunoApi {
            code: "not_found",
            status: 404,
            message: "missing".into(),
            retryable: None,
            details: None,
        }));
        assert!(!prepared_download_is_unavailable(&CliError::SunoApi {
            code: "api_error",
            status: 500,
            message: "unknown outcome".into(),
            retryable: Some(true),
            details: None,
        }));
        assert!(!prepared_download_is_unavailable(&CliError::Download(
            "timed out".into()
        )));
    }

    #[test]
    fn video_fallback_rejects_the_forbidden_sentinel() {
        assert!(!usable_legacy_video_url("/api/forbidden"));
        assert!(!usable_legacy_video_url(
            "https://studio-api-prod.suno.com/api/forbidden?clip=1"
        ));
        assert!(usable_legacy_video_url(
            "https://cdn.example.test/video.mp4"
        ));
    }

    #[tokio::test]
    async fn video_prefers_the_prepared_mp4_route() {
        let server = SequenceServer::start(vec![(
            200,
            r#"{"status":"complete","download_url":"https://cdn.example/video.mp4"}"#.into(),
        )])
        .await;
        let client = server.client();
        let mut target = clip();
        target.video_url = Some("https://legacy.example/video.mp4".into());

        let url = video_download_url(
            &client,
            &target,
            &DownloadAccess {
                source_clip_id: "clip-a".into(),
                authorized_now: false,
                credit_deducted: None,
            },
            super::configured_polling(&context()),
        )
        .await
        .expect("prepared MP4 URL");

        assert_eq!(url, "https://cdn.example/video.mp4");
        let requests = server.captured().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/api/download/clip/clip-a?format=mp4");
    }

    #[tokio::test]
    async fn wav_uses_legacy_get_only_after_prepared_is_explicitly_unavailable() {
        let server = SequenceServer::start(vec![
            (200, r#"{"status":"complete","download_url":null}"#.into()),
            (
                200,
                r#"{"wav_file_url":"https://cdn.example/legacy.wav"}"#.into(),
            ),
        ])
        .await;
        let client = server.client();

        let url = official_download_url(&context(), &client, "clip-a", DownloadFormat::Wav, true)
            .await
            .expect("unlocked legacy WAV URL");

        assert_eq!(url, "https://cdn.example/legacy.wav");
        let requests = server.captured().await;
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/api/download/clip/clip-a?format=wav");
        assert_eq!(requests[1].path, "/api/gen/clip-a/wav_file/");
    }

    #[tokio::test]
    async fn read_only_missing_unlock_proof_never_posts_authorization() {
        let server = SequenceServer::start(vec![(200, source_clip_json(None))]).await;
        let client = server.client();

        let error =
            ensure_download_access("source-1", None, &context_with_read_only(true), &client)
                .await
                .expect_err("read-only must not authorize a locked source");

        assert_eq!(error.error_code(), "download_authorization_required");
        let requests = server.captured().await;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/api/clip/source-1");
    }

    #[tokio::test]
    async fn locked_source_is_reread_then_authorized_once_and_billing_is_refreshed() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (
                200,
                serde_json::json!({
                    "ok": true,
                    "reason": "subscription",
                    "message": "Unlocked",
                    "credit_deducted": true
                })
                .to_string(),
            ),
            (200, billing_json()),
        ])
        .await;
        let client = server.client();

        let (access, warning) =
            ensure_download_access("source-1", Some(false), &context(), &client)
                .await
                .expect("download authorization");

        assert!(access.authorized_now);
        assert_eq!(access.credit_deducted, Some(true));
        assert!(warning.is_none());
        let requests = server.captured().await;
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[0].path, "/api/clip/source-1");
        assert_eq!(requests[1].path, "/api/clip/source-1");
        assert_eq!(requests[2].method, "POST");
        assert_eq!(requests[2].path, "/api/download/authorize");
        assert_eq!(requests[3].path, "/api/billing/info/");
    }

    #[tokio::test]
    async fn shared_parent_access_cache_never_authorizes_each_stem() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (
                200,
                serde_json::json!({
                    "ok": true,
                    "reason": "subscription",
                    "message": "Unlocked",
                    "credit_deducted": false
                })
                .to_string(),
            ),
        ])
        .await;
        let client = server.client();
        let mut cache = HashMap::new();

        let (_, _, first_source_use) =
            cached_download_access(&mut cache, "source-1", None, &context(), &client)
                .await
                .expect("first stem authorizes the parent");
        let (_, _, second_source_use) =
            cached_download_access(&mut cache, "source-1", None, &context(), &client)
                .await
                .expect("second stem reuses parent access");

        assert!(first_source_use);
        assert!(!second_source_use);
        let requests = server.captured().await;
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == "/api/download/authorize")
                .count(),
            1
        );
        assert!(
            requests
                .iter()
                .all(|request| !request.path.contains("stem-"))
        );
    }

    #[tokio::test]
    async fn authorization_rejection_preserves_reason_and_stops_before_file_get() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (
                200,
                serde_json::json!({
                    "ok": false,
                    "reason": "quota_exhausted",
                    "message": "No downloads remaining",
                    "credit_deducted": false
                })
                .to_string(),
            ),
        ])
        .await;
        let client = server.client();

        let error = ensure_download_access("source-1", Some(false), &context(), &client)
            .await
            .expect_err("ok=false must stop before the prepared file request");

        assert_eq!(error.error_code(), "download_authorization_denied");
        let details = error.details().expect("authorization rejection details");
        assert_eq!(details["reason"], "quota_exhausted");
        assert_eq!(details["message"], "No downloads remaining");
        assert_eq!(details["credit_deducted"], false);
        assert_eq!(details["download_started"], false);
        let requests = server.captured().await;
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[2].path, "/api/download/authorize");
    }

    #[tokio::test]
    async fn billing_refresh_failure_warns_without_discarding_authorized_access() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (
                200,
                serde_json::json!({
                    "ok": true,
                    "credit_deducted": true
                })
                .to_string(),
            ),
            (500, r#"{"detail":"billing unavailable"}"#.into()),
        ])
        .await;
        let client = server.client();

        let (access, warning) =
            ensure_download_access("source-1", Some(false), &context(), &client)
                .await
                .expect("billing readback failure must not discard an accepted authorization");

        assert!(access.authorized_now);
        assert_eq!(access.credit_deducted, Some(true));
        let warning = warning.expect("billing refresh warning");
        assert_eq!(warning.field, "download_usage");
        assert!(warning.message.contains("billing"));
        let requests = server.captured().await;
        assert_eq!(requests.len(), 4);
        assert_eq!(requests[3].path, "/api/billing/info/");
    }

    #[tokio::test]
    async fn ambiguous_authorization_is_not_replayed_and_unlock_readback_can_resume() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (500, r#"{"detail":"outcome unknown"}"#.into()),
            (200, source_clip_json(Some(true))),
            (200, billing_json()),
        ])
        .await;
        let client = server.client();

        let (access, warning) =
            ensure_download_access("source-1", Some(false), &context(), &client)
                .await
                .expect("exact unlock readback proves the ambiguous authorization succeeded");

        assert!(access.authorized_now);
        assert_eq!(access.credit_deducted, None);
        assert!(warning.is_none());
        let requests = server.captured().await;
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == "/api/download/authorize")
                .count(),
            1
        );
        assert_eq!(requests[3].path, "/api/clip/source-1");
        assert_eq!(requests[4].path, "/api/billing/info/");
    }

    #[tokio::test]
    async fn redirected_authorization_is_ambiguous_and_triggers_readback_without_replay() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (307, String::new()),
            (200, source_clip_json(Some(false))),
            (200, billing_json()),
        ])
        .await;
        let client = server.client();

        let error = ensure_download_access("source-1", Some(false), &context(), &client)
            .await
            .expect_err("redirected authorization must be reconciled, not exposed for retry");

        assert_eq!(error.error_code(), "ambiguous_mutation");
        let details = error.details().expect("redirect ambiguity details");
        assert_eq!(details["stage"], "response_status");
        assert_eq!(
            details["readback"]["is_download_unlocked"],
            serde_json::json!({"ok": true, "value": false})
        );
        assert_eq!(details["readback"]["billing"]["ok"], true);
        let requests = server.captured().await;
        assert_eq!(requests.len(), 5);
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == "/api/download/authorize")
                .count(),
            1
        );
        assert_eq!(requests[3].path, "/api/clip/source-1");
        assert_eq!(requests[4].path, "/api/billing/info/");
    }

    #[tokio::test]
    async fn unresolved_authorization_ambiguity_preserves_both_readbacks() {
        let server = SequenceServer::start(vec![
            (200, source_clip_json(Some(false))),
            (200, source_clip_json(Some(false))),
            (500, r#"{"detail":"outcome unknown"}"#.into()),
            (200, source_clip_json(Some(false))),
            (200, billing_json()),
        ])
        .await;
        let client = server.client();

        let error = ensure_download_access("source-1", Some(false), &context(), &client)
            .await
            .expect_err("false unlock readback cannot resolve an accepted-state ambiguity");

        assert_eq!(error.error_code(), "ambiguous_mutation");
        let details = error.details().expect("ambiguity details");
        assert_eq!(
            details["readback"]["is_download_unlocked"],
            serde_json::json!({"ok": true, "value": false})
        );
        assert_eq!(details["readback"]["billing"]["ok"], true);
        assert_eq!(
            details["readback"]["billing"]["download_usage"]["current_period_downloads_used"],
            1
        );
        assert!(!details.to_string().contains("must-not-leak"));
        let requests = server.captured().await;
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.path == "/api/download/authorize")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn exact_true_hint_skips_all_authorization_network_calls() {
        let client = crate::api::SunoClient::new_for_tests(
            "http://127.0.0.1:9".into(),
            crate::auth::AuthState {
                jwt: Some("test-jwt".into()),
                ..Default::default()
            },
        )
        .expect("test client");

        let (access, warning) = ensure_download_access(
            "source-1",
            Some(true),
            &context_with_read_only(true),
            &client,
        )
        .await
        .expect("exact unlock proof is sufficient in read-only mode");

        assert!(!access.authorized_now);
        assert!(warning.is_none());
    }

    #[test]
    fn normal_download_ids_are_deduplicated_without_reordering() {
        assert_eq!(
            deduplicate_clip_ids(&[
                "clip-b".into(),
                "clip-a".into(),
                "clip-b".into(),
                "clip-c".into(),
                "clip-a".into(),
            ]),
            vec!["clip-b", "clip-a", "clip-c"]
        );
    }

    #[test]
    fn stems_share_the_parent_download_authorization_source() {
        let target = clip_with_id("stem-1");
        assert_eq!(
            download_source_id(&DownloadSource::EachTarget, &target),
            "stem-1"
        );
        assert_eq!(
            download_source_id(&DownloadSource::SharedParent("parent-1".into()), &target),
            "parent-1"
        );
    }

    #[test]
    fn read_only_missing_unlock_proof_is_structured_and_stops_before_download() {
        let error = read_only_download_authorization_error("source-1", None);
        assert_eq!(error.error_code(), "download_authorization_required");
        let details = error.details().expect("authorization diagnostic details");
        assert_eq!(details["source_clip_id"], "source-1");
        assert_eq!(details["is_download_unlocked"], serde_json::Value::Null);
        assert_eq!(details["authorization_post_sent"], false);
        assert_eq!(details["download_started"], false);
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
                &DownloadAccess {
                    source_clip_id: "clip-a".into(),
                    authorized_now: false,
                    credit_deducted: None,
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

    #[tokio::test]
    async fn batch_preflight_rejects_duplicate_planned_paths_even_with_force() {
        for force in [false, true] {
            let dir = tempfile::tempdir().expect("download output directory");
            let output_dir = dir.path().to_string_lossy().into_owned();
            let error = preflight_download_batch(
                &[
                    clip_with_id("deadbeef-first"),
                    clip_with_id("deadbeef-second"),
                ],
                &output_dir,
                "mp3",
                force,
            )
            .await
            .expect_err("two batch items must never resolve to one destination");

            assert_eq!(error.error_code(), "download_destination_collision");
            let details = error.details().expect("collision details");
            assert_eq!(details["first_clip_id"], "deadbeef-first");
            assert_eq!(details["second_clip_id"], "deadbeef-second");
            assert_eq!(details["download_started"], false);
            assert_eq!(details["authorization_post_sent"], false);
            assert!(!dir.path().join("track-deadbeef.mp3").exists());
        }
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
            &[],
            &[],
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
            &[],
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
    fn first_file_failure_after_authorization_reports_the_remote_side_effect() {
        let authorized = AuthorizedDownloadSource {
            source_clip_id: "source-1".into(),
            credit_deducted: Some(true),
        };
        let warnings = [DownloadWarning {
            clip_id: "source-1".into(),
            field: "download_usage",
            code: "api_error".into(),
            message: "billing refresh failed".into(),
        }];
        let error = partial_download_error(
            &[],
            "clip-failed",
            None,
            &[],
            &[authorized],
            &warnings,
            CliError::Download("CDN failed".into()),
        );

        assert_eq!(error.error_code(), "partial_download");
        let details = error.details().expect("partial download details");
        assert_eq!(details["succeeded"], serde_json::json!([]));
        assert_eq!(
            details["authorized_sources"][0]["source_clip_id"],
            "source-1"
        );
        assert_eq!(details["authorized_sources"][0]["credit_deducted"], true);
        assert_eq!(details["warnings"][0]["field"], "download_usage");
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
