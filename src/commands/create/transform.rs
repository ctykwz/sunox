use crate::app::AppContext;
use crate::cli::{ConcatArgs, CoverArgs, ModelVersion, RemasterArgs, SpeedArgs, StemsArgs};
use crate::core::{AppConfig, CliError};
use crate::output::{self, OutputFormat};

use super::support::{generation_token, output_clips};

pub async fn concat(args: ConcatArgs, ctx: &AppContext) -> Result<(), CliError> {
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
    let clip = client
        .adjust_speed(&args.clip_id, args.multiplier, args.keep_pitch, &title)
        .await?;
    output_clips(&[clip], ctx);
    Ok(())
}

pub async fn stems(args: StemsArgs, ctx: &AppContext) -> Result<(), CliError> {
    let force_captcha = args.captcha && !args.no_captcha;
    let challenge_token = generation_token(args.token.clone(), force_captcha, ctx).await?;
    let clips = ctx
        .client()
        .await?
        .stems(&args.clip_id, challenge_token)
        .await?;
    output_clips(&clips, ctx);
    Ok(())
}
