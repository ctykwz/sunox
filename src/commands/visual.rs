use std::time::Duration;

use crate::api::PollingOptions;
use crate::api::types::{CoverArtBatchDescriptor, CoverArtHistoryRequest, CoverArtPromptImage};
use crate::app::AppContext;
use crate::cli::{
    CoverArtArgs, CoverArtCommand, CoverArtHistoryArgs, CoverArtImageArgs, CoverArtPromptImageArg,
    CoverArtPromptImageKind, CoverArtStatusArgs, CoverArtVideoArgs, GenerateImageArgs,
    GenerateVideoArgs, VideoStatusArgs,
};
use crate::core::{CliError, ensure_poll_timeout_secs};
use crate::output::{self, OutputFormat};
use crate::workflow::visual;

pub async fn generate_image(args: GenerateImageArgs, ctx: &AppContext) -> Result<(), CliError> {
    let clip_id = nonempty("clip ID", args.id)?;
    let prompt = validate_image_prompt(args.prompt)?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = visual::generate_and_apply_clip_image(&client, &clip_id, &prompt).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&result),
        OutputFormat::Table => println!(
            "Applied generated image to clip {clip_id}: {}",
            result.image_url
        ),
    }
    Ok(())
}

pub async fn generate_video(args: GenerateVideoArgs, ctx: &AppContext) -> Result<(), CliError> {
    let clip_id = nonempty("clip ID", args.id)?;
    let polling = polling_options(args.timeout, ctx)?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let status = visual::generate_video_and_wait(&client, &clip_id, polling).await?;
    render_video_status(&clip_id, status, ctx.fmt);
    Ok(())
}

pub async fn video_status(args: VideoStatusArgs, ctx: &AppContext) -> Result<(), CliError> {
    let clip_id = nonempty("clip ID", args.id)?;
    let client = ctx.client().await?;
    let status = if args.wait {
        visual::wait_for_existing_video(&client, &clip_id, polling_options(args.timeout, ctx)?)
            .await?
    } else {
        client.video_generation_status(&clip_id).await?
    };
    render_video_status(&clip_id, status, ctx.fmt);
    Ok(())
}

pub async fn cover_art(args: CoverArtArgs, ctx: &AppContext) -> Result<(), CliError> {
    match args.command {
        CoverArtCommand::Models => cover_art_models(ctx).await,
        CoverArtCommand::Pending => cover_art_pending(ctx).await,
        CoverArtCommand::History(args) => cover_art_history(args, ctx).await,
        CoverArtCommand::Image(args) => cover_art_image(args, ctx).await,
        CoverArtCommand::Video(args) => cover_art_video(args, ctx).await,
        CoverArtCommand::Status(args) => cover_art_status(args, ctx).await,
        CoverArtCommand::ApplyImage(args) => {
            let clip_id = nonempty("clip ID", args.id)?;
            let batch_id = nonempty("batch ID", args.batch_id)?;
            let image_id = nonempty("generated image ID", args.image_id)?;
            let (client, _mutation_guard) = ctx.mutation_client().await?;
            let applied =
                visual::apply_cover_art_image(&client, &clip_id, &batch_id, &image_id).await?;
            render_serializable(&applied, ctx.fmt, || {
                format!("Applied generated image {image_id} to clip {clip_id}")
            })
        }
        CoverArtCommand::ApplyVideo(args) => {
            let clip_id = nonempty("clip ID", args.id)?;
            let batch_id = nonempty("batch ID", args.batch_id)?;
            let video_upload_id = nonempty("video upload ID", args.video_upload_id)?;
            let (client, _mutation_guard) = ctx.mutation_client().await?;
            let applied =
                visual::apply_cover_art_video(&client, &clip_id, &batch_id, &video_upload_id)
                    .await?;
            render_serializable(&applied, ctx.fmt, || {
                format!("Applied generated video {video_upload_id} to clip {clip_id}")
            })
        }
    }
}

async fn cover_art_models(ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let models = client.cover_art_model_configs().await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(models),
        OutputFormat::Table => {
            for model in &models.image_model_categories {
                println!(
                    "image\t{}\t{}",
                    model.category,
                    model.display_name.as_deref().unwrap_or("")
                );
            }
            for model in &models.video_model_categories {
                println!(
                    "video\t{}\t{}\t{:?}",
                    model.category,
                    model.display_name.as_deref().unwrap_or(""),
                    model.allowed_durations
                );
            }
        }
    }
    Ok(())
}

async fn cover_art_pending(ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let pending = client.pending_cover_art_batches().await?;
    render_serializable(&pending, ctx.fmt, || {
        format!("{} pending cover-art batch(es)", pending.batch_ids.len())
    })
}

async fn cover_art_history(args: CoverArtHistoryArgs, ctx: &AppContext) -> Result<(), CliError> {
    let request = CoverArtHistoryRequest {
        clip_id: optional_nonempty("clip ID", args.clip_id)?,
        created_at_offset: optional_nonempty("history cursor", args.cursor)?,
        favorites_only: args.favorites,
        media_type: args.media.map(|media| media.as_protocol().into()),
        limit: args.limit,
    };
    let client = ctx.client().await?;
    let history = client.cover_art_history(&request).await?;
    render_serializable(&history, ctx.fmt, || {
        format!("{} cover-art history batch(es)", history.history.len())
    })
}

async fn cover_art_image(args: CoverArtImageArgs, ctx: &AppContext) -> Result<(), CliError> {
    let clip_id = nonempty("clip ID", args.id)?;
    let prompt = validate_batch_prompt(args.prompt, args.prompt_image.is_some())?;
    let prompt_image = args.prompt_image.map(protocol_prompt_image).transpose()?;
    let polling = (!args.no_wait)
        .then(|| polling_options(args.timeout, ctx))
        .transpose()?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = visual::generate_cover_art_image_batch(
        &client,
        &clip_id,
        &prompt,
        args.model.as_deref(),
        prompt_image,
        polling,
    )
    .await?;
    render_serializable(&result, ctx.fmt, || {
        format!(
            "Submitted image cover-art batch {} (cost {})",
            result.submission.batch_id, result.cost.cost
        )
    })
}

async fn cover_art_video(args: CoverArtVideoArgs, ctx: &AppContext) -> Result<(), CliError> {
    let clip_id = nonempty("clip ID", args.id)?;
    let prompt = validate_batch_prompt(args.prompt, args.start_image.is_some())?;
    let start_image = args.start_image.map(protocol_prompt_image).transpose()?;
    let polling = (!args.no_wait)
        .then(|| polling_options(args.timeout, ctx))
        .transpose()?;
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = visual::generate_cover_art_video_batch(
        &client,
        &clip_id,
        &prompt,
        args.model.as_deref(),
        args.duration,
        start_image,
        polling,
    )
    .await?;
    render_serializable(&result, ctx.fmt, || {
        format!(
            "Submitted video cover-art batch {} (cost {})",
            result.submission.batch_id, result.cost.cost
        )
    })
}

async fn cover_art_status(args: CoverArtStatusArgs, ctx: &AppContext) -> Result<(), CliError> {
    let descriptor = CoverArtBatchDescriptor::new(
        nonempty("batch ID", args.batch_id)?,
        args.media.as_protocol(),
    );
    let client = ctx.client().await?;
    let response = if args.wait {
        visual::wait_for_cover_art_batch(&client, &descriptor, polling_options(args.timeout, ctx)?)
            .await?
    } else {
        client
            .poll_cover_art_batches(std::slice::from_ref(&descriptor))
            .await?
    };
    render_serializable(&response, ctx.fmt, || {
        format!("Cover-art batch {}", descriptor.id)
    })
}

fn validate_batch_prompt(prompt: Option<String>, has_image: bool) -> Result<String, CliError> {
    let prompt = prompt.unwrap_or_default();
    if prompt.trim().is_empty() && !has_image {
        return Err(CliError::Config(
            "cover-art generation requires --prompt or a prompt image".into(),
        ));
    }
    if prompt.encode_utf16().count() > 800 {
        return Err(CliError::Config(
            "cover-art prompt must not exceed 800 UTF-16 code units".into(),
        ));
    }
    Ok(prompt)
}

fn protocol_prompt_image(value: CoverArtPromptImageArg) -> Result<CoverArtPromptImage, CliError> {
    let id = nonempty("prompt image ID", value.id)?;
    Ok(match value.kind {
        CoverArtPromptImageKind::Uploaded => CoverArtPromptImage::uploaded(id),
        CoverArtPromptImageKind::Generated => CoverArtPromptImage::generated(id),
        CoverArtPromptImageKind::S3Filename => CoverArtPromptImage::s3_filename(id),
    })
}

fn optional_nonempty(label: &str, value: Option<String>) -> Result<Option<String>, CliError> {
    value.map(|value| nonempty(label, value)).transpose()
}

fn render_serializable<T: serde::Serialize>(
    value: &T,
    format: OutputFormat,
    table: impl FnOnce() -> String,
) -> Result<(), CliError> {
    match format {
        OutputFormat::Json => output::json::success(value),
        OutputFormat::Table => println!("{}", table()),
    }
    Ok(())
}

fn polling_options(timeout: Option<u64>, ctx: &AppContext) -> Result<PollingOptions, CliError> {
    let timeout = timeout.unwrap_or(ctx.config.poll_timeout_secs);
    ensure_poll_timeout_secs(timeout)?;
    Ok(PollingOptions {
        timeout: Duration::from_secs(timeout),
        interval: Duration::from_secs(ctx.config.poll_interval_secs.max(1)),
    })
}

fn nonempty(label: &str, value: String) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CliError::Config(format!("{label} must not be empty")));
    }
    Ok(value.to_string())
}

fn validate_image_prompt(prompt: String) -> Result<String, CliError> {
    let prompt = nonempty("image prompt", prompt)?;
    if prompt.chars().count() > 200 {
        return Err(CliError::Config(
            "image prompt must not exceed the current Web limit of 200 Unicode characters".into(),
        ));
    }
    Ok(prompt)
}

fn render_video_status(
    clip_id: &str,
    status: crate::api::types::VideoGenerationStatus,
    format: OutputFormat,
) {
    match format {
        OutputFormat::Json => output::json::success(status),
        OutputFormat::Table => {
            println!("{clip_id}\t{}", status.status);
            if let Some(url) = status.video_url {
                println!("{url}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_image_prompt;

    #[test]
    fn image_prompt_rejects_blank_and_overlong_input_before_mutation() {
        assert!(validate_image_prompt("  ".into()).is_err());
        assert!(validate_image_prompt("画".repeat(201)).is_err());
        assert!(validate_image_prompt("画".repeat(200)).is_ok());
    }
}
