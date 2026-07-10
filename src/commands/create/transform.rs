use crate::app::AppContext;
use crate::cli::{
    ConcatArgs, CoverArgs, CropArgs, FadeArgs, ModelVersion, RemasterArgs, ReverseArgs, SpeedArgs,
    StemsArgs,
};
use crate::core::{AppConfig, CliError};
use crate::output::{self, OutputFormat};

use super::support::{generation_token, output_clips};

pub async fn concat(args: ConcatArgs, ctx: &AppContext) -> Result<(), CliError> {
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let clip = ctx.client().await?.concat(&args.clip_id).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&clip),
        OutputFormat::Table => output::table::clips(&[clip]),
    }
    Ok(())
}

pub async fn cover(args: CoverArgs, ctx: &AppContext) -> Result<(), CliError> {
    let model = cover_model_api_key(args.model.as_ref(), &ctx.config);
    if !ctx.quiet {
        eprintln!(
            "Creating cover ({})...",
            cover_model_label(args.model.as_ref(), &ctx.config)
        );
    }
    let force_captcha = args.captcha && !args.no_captcha;
    let challenge_token = generation_token(args.token.clone(), force_captcha, ctx).await?;
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let client = ctx.client().await?;
    let clips = client
        .cover(&args.clip_id, model, args.tags.as_deref(), challenge_token)
        .await?;
    output_clips(&clips, ctx);
    Ok(())
}

fn cover_model_api_key<'a>(model: Option<&'a ModelVersion>, config: &'a AppConfig) -> &'a str {
    model
        .map(ModelVersion::to_api_key)
        .unwrap_or(config.default_model.as_str())
}

fn cover_model_label<'a>(model: Option<&'a ModelVersion>, config: &'a AppConfig) -> &'a str {
    model
        .map(ModelVersion::display_name)
        .unwrap_or(config.default_model.as_str())
}

pub async fn remaster(args: RemasterArgs, ctx: &AppContext) -> Result<(), CliError> {
    if !ctx.quiet {
        eprintln!("Remastering with {}...", args.model.to_api_key());
    }
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let client = ctx.client().await?;
    let clips = client
        .remaster(&args.clip_id, args.model.to_api_key())
        .await?;
    output_clips(&clips, ctx);
    Ok(())
}

pub async fn speed(args: SpeedArgs, ctx: &AppContext) -> Result<(), CliError> {
    if !args.multiplier.is_finite() || args.multiplier <= 0.0 {
        return Err(CliError::Config(
            "--multiplier must be a positive finite number".into(),
        ));
    }

    let client = ctx.client().await?;
    let title = match args.title {
        Some(title) => title,
        None => {
            let requested = [args.clip_id.clone()];
            let source = client
                .get_clips(&requested)
                .await?
                .into_iter()
                .find(|clip| clip.id == args.clip_id)
                .ok_or_else(|| CliError::NotFound(format!("clip: {}", args.clip_id)))?;
            format!("{} ({:.2}x)", source.title, args.multiplier)
        }
    };
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let clip = client
        .adjust_speed(&args.clip_id, args.multiplier, args.keep_pitch, &title)
        .await?;
    output_clips(&[clip], ctx);
    Ok(())
}

pub async fn reverse(args: ReverseArgs, ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let title = match args.title {
        Some(title) => title,
        None => {
            let source = require_source_clip(&client, &args.clip_id).await?;
            format!("{} (Reversed)", source.title)
        }
    };
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let clip = client.reverse_clip(&args.clip_id, &title).await?;
    output_clips(&[clip], ctx);
    Ok(())
}

pub async fn crop(args: CropArgs, ctx: &AppContext) -> Result<(), CliError> {
    if !args.start.is_finite()
        || !args.end.is_finite()
        || args.start < 0.0
        || args.end <= args.start
    {
        return Err(CliError::Config(
            "--start and --end must be finite seconds with 0 <= start < end".into(),
        ));
    }

    let client = ctx.client().await?;
    let title = match args.title {
        Some(title) => title,
        None => {
            let source = require_source_clip(&client, &args.clip_id).await?;
            let suffix = if args.remove_section {
                "Remove Section"
            } else {
                "Crop"
            };
            format!("{} ({suffix})", source.title)
        }
    };
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let polling = configured_polling(ctx);
    let clip = client
        .crop_clip(
            &args.clip_id,
            args.start,
            args.end,
            args.remove_section,
            &title,
            polling,
        )
        .await?;
    output_clips(&[clip], ctx);
    Ok(())
}

pub async fn fade(args: FadeArgs, ctx: &AppContext) -> Result<(), CliError> {
    if args.fade_in.is_none() && args.fade_out.is_none() {
        return Err(CliError::Config(
            "provide --in <seconds>, --out <seconds>, or both".into(),
        ));
    }
    if args
        .fade_in
        .into_iter()
        .chain(args.fade_out)
        .any(|value| !value.is_finite() || value < 0.0)
    {
        return Err(CliError::Config(
            "fade times must be finite non-negative seconds".into(),
        ));
    }

    let client = ctx.client().await?;
    let title = match args.title {
        Some(title) => title,
        None => {
            let source = require_source_clip(&client, &args.clip_id).await?;
            let suffix = match (args.fade_in.is_some(), args.fade_out.is_some()) {
                (true, true) => "Fade",
                (true, false) => "Fade In",
                (false, true) => "Fade Out",
                (false, false) => unreachable!("validated above"),
            };
            format!("{} ({suffix})", source.title)
        }
    };
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let polling = configured_polling(ctx);
    let clip = client
        .fade_clip(&args.clip_id, args.fade_in, args.fade_out, &title, polling)
        .await?;
    output_clips(&[clip], ctx);
    Ok(())
}

fn configured_polling(ctx: &AppContext) -> crate::api::PollingOptions {
    crate::api::PollingOptions {
        timeout: std::time::Duration::from_secs(ctx.config.poll_timeout_secs),
        interval: std::time::Duration::from_secs(ctx.config.poll_interval_secs.max(1)),
    }
}

async fn require_source_clip(
    client: &crate::api::SunoClient,
    clip_id: &str,
) -> Result<crate::api::types::Clip, CliError> {
    let requested = [clip_id.to_string()];
    client
        .get_clips(&requested)
        .await?
        .into_iter()
        .find(|clip| clip.id == clip_id)
        .ok_or_else(|| CliError::NotFound(format!("clip: {clip_id}")))
}

pub async fn stems(args: StemsArgs, ctx: &AppContext) -> Result<(), CliError> {
    let force_captcha = args.captcha && !args.no_captcha;
    let challenge_token = generation_token(args.token.clone(), force_captcha, ctx).await?;
    let _mutation_guard = ctx.acquire_mutation_lock()?;
    let clips = ctx
        .client()
        .await?
        .stems(&args.clip_id, challenge_token)
        .await?;
    output_clips(&clips, ctx);
    Ok(())
}
