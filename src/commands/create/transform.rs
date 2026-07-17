use crate::app::AppContext;
use crate::cli::{
    ConcatArgs, CoverArgs, CoverModel, CropArgs, FadeArgs, RemasterArgs, ReverseArgs, SpeedArgs,
    StemsArgs,
};
use crate::core::{AppConfig, CliError};

use super::support::{execute_generation_submission, output_clips};

pub async fn concat(args: ConcatArgs, ctx: &AppContext) -> Result<(), CliError> {
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let clip = client.concat(&args.clip_id).await?;
    output_clips(&[clip], ctx);
    Ok(())
}

pub async fn cover(args: CoverArgs, ctx: &AppContext) -> Result<(), CliError> {
    let model = cover_model_api_key(args.model.as_ref(), &ctx.config)?;
    if !ctx.quiet {
        eprintln!(
            "Creating cover ({})...",
            cover_model_label(args.model.as_ref(), &ctx.config)
        );
    }
    let force_captcha = args.captcha && !args.no_captcha;
    let token = args.token.clone();
    let model = model.to_string();
    let clips = execute_generation_submission(
        token,
        force_captcha,
        ctx,
        move || async move {
            let client = ctx.client().await?;
            let mut req = client
                .prepare_cover_request(&args.clip_id, &model, args.tags.as_deref(), None)
                .await?;
            client.prepare_generation_request(&mut req).await?;
            Ok((client, req))
        },
        |(client, mut req), challenge_token| async move {
            req.set_challenge_token(challenge_token);
            client.submit_prepared_generation(&req).await
        },
    )
    .await?;
    output_clips(&clips, ctx);
    Ok(())
}

fn cover_model_api_key<'a>(
    model: Option<&'a CoverModel>,
    config: &'a AppConfig,
) -> Result<&'a str, CliError> {
    if let Some(model) = model {
        return Ok(model.to_api_key());
    }
    match config.default_model.as_str() {
        "auto" => Ok("auto"),
        "chirp-auk-turbo" => Err(CliError::Config(
            "default_model v4.5-all is not verified for cover generation; pass an explicitly supported `sunox clip cover --model` value".into(),
        )),
        model => Ok(model),
    }
}

fn cover_model_label<'a>(model: Option<&'a CoverModel>, config: &'a AppConfig) -> &'a str {
    model.map(CoverModel::display_name).unwrap_or_else(|| {
        if config.default_model == "auto" {
            "account default"
        } else {
            config.default_model.as_str()
        }
    })
}

pub async fn remaster(args: RemasterArgs, ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let model = match client.billing_info().await {
        Ok(info) => select_remaster_model(&info.remaster_model_types, args.model.as_ref())?,
        Err(error) if error.is_auth_or_rate_limit() => return Err(error),
        Err(_) => args
            .model
            .as_ref()
            .map(|model| model.to_api_key().to_string())
            .unwrap_or_else(|| "chirp-flounder".into()),
    };
    if !ctx.quiet {
        eprintln!("Remastering with {model}...");
    }
    let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
    let clips = client
        .remaster(&args.clip_id, &model, args.variation)
        .await?;
    output_clips(&clips, ctx);
    Ok(())
}

fn select_remaster_model(
    models: &[crate::api::types::RemasterModelInfo],
    requested: Option<&crate::cli::RemasterModel>,
) -> Result<String, CliError> {
    if models.is_empty() {
        return Err(CliError::Config(
            "Suno billing info returned no remaster models; refusing to guess after a successful account capability lookup".into(),
        ));
    }
    let selected = if let Some(requested) = requested {
        models
            .iter()
            .find(|model| model.external_key == requested.to_api_key())
    } else {
        models
            .iter()
            .find(|model| model.is_default_model && model.can_use != Some(false))
            .or_else(|| models.iter().find(|model| model.can_use != Some(false)))
    };
    let selected = selected.ok_or_else(|| {
        let requested = requested
            .map(|model| model.display_name())
            .unwrap_or("an account default remaster model");
        CliError::Config(format!(
            "Suno account does not report {requested} as an available remaster model; run `sunox models --json`"
        ))
    })?;
    if selected.can_use == Some(false) {
        return Err(CliError::Config(format!(
            "Suno account reports remaster model {} as unavailable",
            selected.external_key
        )));
    }
    Ok(selected.external_key.clone())
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
    let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
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
    let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
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
    let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
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
    let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
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
    let token = args.token.clone();
    let clips = execute_generation_submission(
        token,
        force_captcha,
        ctx,
        move || async move {
            let client = ctx.client().await?;
            let mut req = client.prepare_stems_request(&args.clip_id, None).await?;
            client.prepare_generation_request(&mut req).await?;
            Ok((client, req))
        },
        |(client, mut req), challenge_token| async move {
            req.set_challenge_token(challenge_token);
            client.submit_prepared_generation(&req).await
        },
    )
    .await?;
    output_clips(&clips, ctx);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::api::types::RemasterModelInfo;
    use crate::cli::RemasterModel;

    use super::select_remaster_model;

    #[test]
    fn remaster_auto_uses_account_default_without_treating_unknown_as_unavailable() {
        let models = vec![RemasterModelInfo {
            name: "v5".into(),
            external_key: "chirp-carp".into(),
            is_default_model: true,
            can_use: None,
        }];

        assert_eq!(
            select_remaster_model(&models, None).expect("account default"),
            "chirp-carp"
        );
    }

    #[test]
    fn remaster_rejects_an_explicitly_unavailable_model() {
        let models = vec![RemasterModelInfo {
            name: "v5.5".into(),
            external_key: "chirp-flounder".into(),
            is_default_model: true,
            can_use: Some(false),
        }];

        assert!(select_remaster_model(&models, Some(&RemasterModel::V55)).is_err());
    }
}
