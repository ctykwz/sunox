use crate::app::AppContext;
use crate::cli::{
    ConcatArgs, CoverArgs, CropArgs, FadeArgs, RemasterArgs, ReverseArgs, SpeedArgs, StemGroup,
    StemMode, StemsArgs,
};
use crate::core::{AppConfig, CliError, normalize_generation_model_selector};

use super::support::{
    ChallengeMode, execute_generation_submission, output_clips, output_generation,
};

pub async fn concat(args: ConcatArgs, ctx: &AppContext) -> Result<(), CliError> {
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = client.concat(&args.clip_id).await?;
    output_generation(&result, ctx);
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
    let challenge_mode = ChallengeMode::from_flags(args.captcha, args.no_captcha);
    let token = args.token.clone();
    let clips = execute_generation_submission(token, challenge_mode, ctx, move || async move {
        let client = ctx.client().await?;
        let mut req = client
            .prepare_cover_request(&args.clip_id, &model, args.tags.as_deref(), None)
            .await?;
        client.prepare_generation_request(&mut req).await?;
        Ok((client, req))
    })
    .await?;
    output_generation(&clips, ctx);
    Ok(())
}

fn cover_model_api_key(model: Option<&String>, config: &AppConfig) -> Result<String, CliError> {
    normalize_generation_model_selector(
        model
            .map(String::as_str)
            .unwrap_or(config.default_model.as_str()),
    )
}

fn cover_model_label<'a>(model: Option<&'a String>, config: &'a AppConfig) -> &'a str {
    model.map(String::as_str).unwrap_or_else(|| {
        if config.default_model == "auto" {
            "account default"
        } else {
            config.default_model.as_str()
        }
    })
}

pub async fn remaster(args: RemasterArgs, ctx: &AppContext) -> Result<(), CliError> {
    let client = ctx.client().await?;
    let model = resolve_remaster_model(client.billing_info().await, args.model.as_ref())?;
    if !ctx.quiet {
        eprintln!("Remastering with {model}...");
    }
    let _mutation_guard = ctx.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
    let result = client
        .remaster(&args.clip_id, &model, args.variation)
        .await?;
    output_generation(&result, ctx);
    Ok(())
}

fn resolve_remaster_model(
    billing: Result<crate::api::types::BillingInfo, CliError>,
    requested: Option<&crate::cli::RemasterModel>,
) -> Result<String, CliError> {
    let info = billing?;
    ensure_remaster_plan_access(&info)?;
    select_remaster_model(&info.remaster_model_types, requested)
}

fn ensure_remaster_plan_access(info: &crate::api::types::BillingInfo) -> Result<(), CliError> {
    let has_access = info
        .accessible_features
        .as_ref()
        .is_some_and(|features| features.contains("remaster"));
    if !has_access {
        return Err(CliError::Config(
            "Suno does not expose Remaster in the current account's accessible_features".into(),
        ));
    }
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
            .find(|model| crate::cli::RemasterModel::supports_api_key(&model.external_key))
    };
    let selected = selected.ok_or_else(|| {
        if let Some(requested) = requested {
            return CliError::Config(format!(
                "Suno account does not report {} as an available remaster model; run `sunox models --json`",
                requested.display_name()
            ));
        }
        let reported = models
            .iter()
            .map(|model| model.external_key.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        CliError::Config(format!(
            "Suno account reports no supported remaster models; reported keys: {reported}. Supported keys are chirp-flounder, chirp-carp, and chirp-bass"
        ))
    })?;
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
    let stem = validate_stem_mode(args.mode, args.stem)?;
    if !ctx.quiet {
        match args.mode {
            StemMode::Auto => eprintln!(
                "Starting Pro Auto Split (up to 12 stems; Suno currently charges 50 credits)..."
            ),
            StemMode::Split => eprintln!(
                "Starting Pro Split from Mix for `{}` (target + complement; Suno currently charges 20 credits total)...",
                stem.expect("validated split stem").canonical_name()
            ),
        }
    }
    let challenge_mode = ChallengeMode::from_flags(args.captcha, args.no_captcha);
    let token = args.token.clone();
    let mode = args.mode;
    let clips = execute_generation_submission(token, challenge_mode, ctx, move || async move {
        let client = ctx.client().await?;
        let mut req = match mode {
            StemMode::Auto => client.prepare_stems_request(&args.clip_id, None).await?,
            StemMode::Split => {
                client
                    .prepare_split_stems_request(
                        &args.clip_id,
                        stem.expect("validated split stem").api_group(),
                        stem.expect("validated split stem").canonical_name(),
                        None,
                    )
                    .await?
            }
        };
        client.prepare_generation_request(&mut req).await?;
        Ok((client, req))
    })
    .await?;
    output_generation(&clips, ctx);
    Ok(())
}

fn validate_stem_mode(
    mode: StemMode,
    stem: Option<StemGroup>,
) -> Result<Option<StemGroup>, CliError> {
    match (mode, stem) {
        (StemMode::Auto, None) => Ok(None),
        (StemMode::Auto, Some(_)) => Err(CliError::Config(
            "--stem is only valid with --mode split".into(),
        )),
        (StemMode::Split, Some(stem)) => Ok(Some(stem)),
        (StemMode::Split, None) => Err(CliError::Config(
            "--mode split requires --stem with a current Pro target group".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use crate::api::types::RemasterModelInfo;
    use crate::cli::{RemasterModel, StemGroup, StemMode};
    use crate::core::CliError;

    use super::{
        ensure_remaster_plan_access, resolve_remaster_model, select_remaster_model,
        validate_stem_mode,
    };

    fn billing_fixture(
        accessible_features: Option<serde_json::Value>,
    ) -> crate::api::types::BillingInfo {
        let mut value = serde_json::json!({
            "credits": 100,
            "total_credits_left": 100,
            "monthly_usage": 0,
            "monthly_limit": 2500,
            "is_active": true,
            "plan": {
                "name": "Pro Plan",
                "plan_key": "pro",
                "usage_plan_features": [{"name": "remaster"}]
            },
            "models": [],
            "period": "month",
            "renews_on": null,
            "remaster_model_types": [{
                "name": "v5.5",
                "external_key": "chirp-flounder",
                "is_default_model": true,
                "can_use": false
            }]
        });
        if let Some(features) = accessible_features {
            value["accessible_features"] = features;
        }
        serde_json::from_value(value).expect("billing fixture")
    }

    #[test]
    fn remaster_auto_uses_first_supported_web_model_without_filtering_can_use() {
        let models = vec![
            RemasterModelInfo {
                name: "future".into(),
                external_key: "chirp-future".into(),
                is_default_model: true,
                can_use: Some(true),
                extra: Default::default(),
            },
            RemasterModelInfo {
                name: "v5".into(),
                external_key: "chirp-carp".into(),
                is_default_model: false,
                can_use: Some(false),
                extra: Default::default(),
            },
            RemasterModelInfo {
                name: "v5.5".into(),
                external_key: "chirp-flounder".into(),
                is_default_model: true,
                can_use: Some(true),
                extra: Default::default(),
            },
        ];

        assert_eq!(
            select_remaster_model(&models, None).expect("first supported Web-listed model"),
            "chirp-carp"
        );
    }

    #[test]
    fn remaster_auto_rejects_an_account_list_with_only_unknown_models() {
        let models = vec![RemasterModelInfo {
            name: "future".into(),
            external_key: "chirp-future".into(),
            is_default_model: true,
            can_use: Some(true),
            extra: Default::default(),
        }];

        let error = select_remaster_model(&models, None)
            .expect_err("unknown Remaster payloads must not be guessed");

        assert!(error.to_string().contains("supported remaster models"));
        assert!(error.to_string().contains("chirp-future"));
    }

    #[test]
    fn remaster_uses_a_web_listed_model_even_when_legacy_can_use_is_false() {
        let models = vec![RemasterModelInfo {
            name: "v5.5".into(),
            external_key: "chirp-flounder".into(),
            is_default_model: true,
            can_use: Some(false),
            extra: Default::default(),
        }];

        assert_eq!(
            select_remaster_model(&models, Some(&RemasterModel::V55))
                .expect("current Web lists the model without filtering can_use"),
            "chirp-flounder"
        );
    }

    #[test]
    fn remaster_plan_access_uses_current_top_level_features() {
        let pro = billing_fixture(Some(serde_json::json!(["remaster"])));
        ensure_remaster_plan_access(&pro).expect("Pro Remaster feature");

        let legacy_pro = billing_fixture(Some(serde_json::json!({"remaster": true})));
        ensure_remaster_plan_access(&legacy_pro).expect("legacy Pro Remaster feature");

        let basic = billing_fixture(Some(serde_json::json!(["tag_upsample"])));
        let error = ensure_remaster_plan_access(&basic)
            .expect_err("current accessible features must gate Remaster");
        assert!(error.to_string().contains("accessible_features"));
    }

    #[test]
    fn remaster_plan_access_fails_closed_when_current_top_level_features_are_missing_or_empty() {
        for info in [
            billing_fixture(None),
            billing_fixture(Some(serde_json::json!([]))),
            billing_fixture(Some(serde_json::json!({"remaster": false}))),
            billing_fixture(Some(serde_json::json!("remaster"))),
        ] {
            let error = ensure_remaster_plan_access(&info)
                .expect_err("unknown or disabled feature shapes must not authorize Remaster");
            assert!(error.to_string().contains("accessible_features"));
        }
    }

    #[test]
    fn remaster_propagates_billing_errors_instead_of_guessing() {
        let http_error = CliError::SunoApi {
            code: "server_error",
            status: 500,
            message: "billing unavailable".into(),
            retryable: Some(true),
            details: None,
        };
        assert!(matches!(
            resolve_remaster_model(Err(http_error), None),
            Err(CliError::SunoApi { status: 500, .. })
        ));

        let schema_error =
            CliError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err());
        assert!(matches!(
            resolve_remaster_model(Err(schema_error), Some(&RemasterModel::V55)),
            Err(CliError::Json(_))
        ));

        let transport_error = CliError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "billing transport unavailable",
        ));
        assert!(matches!(
            resolve_remaster_model(Err(transport_error), None),
            Err(CliError::Io(_))
        ));
    }

    #[test]
    fn split_from_mix_requires_a_non_empty_target() {
        assert_eq!(
            validate_stem_mode(StemMode::Split, Some(StemGroup::Vocals)).expect("target"),
            Some(StemGroup::Vocals)
        );
        let error =
            validate_stem_mode(StemMode::Split, None).expect_err("split target must be explicit");
        assert!(error.to_string().contains("--stem"));
    }

    #[test]
    fn auto_split_rejects_a_target_to_avoid_a_mixed_contract() {
        assert_eq!(
            validate_stem_mode(StemMode::Auto, None).expect("auto split"),
            None
        );
        let error = validate_stem_mode(StemMode::Auto, Some(StemGroup::Vocals))
            .expect_err("auto split cannot send stem_name");
        assert!(error.to_string().contains("--mode split"));
    }

    #[test]
    fn pro_stem_groups_map_to_current_canonical_default_names() {
        for (group, api_group, canonical) in [
            (StemGroup::Vocals, "Vocals", "Lead Vocal"),
            (StemGroup::BackingVocals, "Backing_Vocals", "Backing Vocals"),
            (StemGroup::Drums, "Drums", "Drum Kit"),
            (StemGroup::Bass, "Bass", "Bass"),
            (StemGroup::Guitar, "Guitar", "Guitar"),
            (StemGroup::Keyboard, "Keyboard", "Keyboards"),
            (StemGroup::Percussion, "Percussion", "Percussion"),
            (StemGroup::Strings, "Strings", "String Section"),
            (StemGroup::Synth, "Synth", "Synth"),
            (StemGroup::Fx, "FX", "Sound Effects"),
            (StemGroup::Brass, "Brass", "Brass Section"),
            (StemGroup::Woodwinds, "Woodwinds", "Woodwinds"),
        ] {
            assert_eq!(group.api_group(), api_group);
            assert_eq!(group.canonical_name(), canonical);
        }
    }
}
