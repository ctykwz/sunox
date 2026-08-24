use crate::api::SunoClient;
use crate::api::extend::ExtendClipOptions;
use crate::api::generate::{TAG_UPSAMPLE_FEATURE, validate_generation_lengths_with_limits};
use crate::api::types::{GenerateRequest, LastTagsGeneration};
use crate::app::AppContext;
use crate::cli::{CreateArgs, DescribeArgs, ExtendArgs, GenerateArgs};
use crate::core::{AppConfig, CliError, normalize_generation_model_selector};
use crate::workflow::generation::{build_control_sliders, build_tags};

use super::support::{ChallengeMode, execute_generation_submission, output_generation};

pub async fn create(args: CreateArgs, ctx: &AppContext) -> Result<(), CliError> {
    validate_create_lyrics_project_mode(&args)?;
    if args.instrumental || args.lyrics.is_some() || args.lyrics_file.is_some() {
        return generate(build_generate_args_from_create(args), ctx).await;
    }

    describe(build_describe_args_from_create(args)?, ctx).await
}

fn validate_create_lyrics_project_mode(args: &CreateArgs) -> Result<(), CliError> {
    if args.lyrics_project_id.is_some()
        && (args.instrumental || (args.lyrics.is_none() && args.lyrics_file.is_none()))
    {
        return Err(CliError::Config(
            "--lyrics-project-id requires explicit custom lyrics from --lyrics or --lyrics-file and cannot be used for description or unconstrained instrumental mode"
                .into(),
        ));
    }
    Ok(())
}

fn build_describe_args_from_create(args: CreateArgs) -> Result<DescribeArgs, CliError> {
    let prompt = args
        .prompt
        .ok_or_else(|| CliError::Config("provide a prompt or --lyrics/--lyrics-file".into()))?;
    Ok(DescribeArgs {
        title: args.title,
        prompt,
        tags: args.tags,
        exclude: args.exclude,
        model: args.model,
        duration: args.duration,
        vocal: args.vocal,
        weirdness: args.weirdness,
        style_influence: args.style_influence,
        enhance_tags: args.enhance_tags,
        instrumental: args.instrumental,
        token: args.token,
        captcha: args.captcha,
        no_captcha: args.no_captcha,
        persona: args.persona,
    })
}

pub(crate) fn build_generate_args_from_create(args: CreateArgs) -> GenerateArgs {
    let tags = if args.instrumental {
        merge_instrumental_prompt_and_tags(args.prompt, args.tags)
    } else {
        args.tags
    };

    GenerateArgs {
        title: args.title,
        tags,
        exclude: args.exclude,
        lyrics: args.lyrics,
        lyrics_file: args.lyrics_file,
        lyrics_project_id: args.lyrics_project_id,
        model: args.model,
        duration: args.duration,
        vocal: if args.instrumental { None } else { args.vocal },
        weirdness: args.weirdness,
        style_influence: args.style_influence,
        enhance_tags: args.enhance_tags,
        instrumental: args.instrumental,
        token: args.token,
        captcha: args.captcha,
        no_captcha: args.no_captcha,
        persona: args.persona,
    }
}

fn merge_instrumental_prompt_and_tags(
    prompt: Option<String>,
    tags: Option<String>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(tags) = tags.and_then(non_empty) {
        parts.push(tags);
    }
    if let Some(prompt) = prompt.and_then(non_empty) {
        parts.push(prompt);
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else if trimmed.len() == value.len() {
        Some(value)
    } else {
        Some(trimmed.to_string())
    }
}

async fn generate(args: GenerateArgs, ctx: &AppContext) -> Result<(), CliError> {
    let mut req = build_generate_request(&args, &ctx.config)?;
    let challenge_mode = ChallengeMode::from_flags(args.captcha, args.no_captcha);
    let token = args.token.clone();
    let should_enhance_tags = args.enhance_tags;
    let instrumental = args.instrumental;

    if !ctx.quiet {
        let persona_note = if args.persona.is_some() {
            " with voice persona"
        } else {
            ""
        };
        eprintln!(
            "Submitting generation ({}{persona_note})...",
            model_label(args.model.as_ref(), &ctx.config)
        );
    }
    let clips = execute_generation_submission(token, challenge_mode, ctx, move || async move {
        let client = ctx.client().await?;
        validate_lyrics_project_reference(&req, &client).await?;
        resolve_persona_reference(&mut req, &client).await?;
        if should_enhance_tags {
            let limits = client
                .prepare_generation_request_with_features(&mut req, &[TAG_UPSAMPLE_FEATURE])
                .await?;
            enhance_tags(&mut req, instrumental, &client).await?;
            validate_generation_lengths_with_limits(&req, &limits)?;
        } else {
            client.prepare_generation_request(&mut req).await?;
        }
        Ok((client, req))
    })
    .await?;
    output_generation(&clips, ctx);
    Ok(())
}

pub(crate) fn build_generate_request(
    args: &GenerateArgs,
    config: &AppConfig,
) -> Result<GenerateRequest, CliError> {
    if args.instrumental && (args.lyrics.is_some() || args.lyrics_file.is_some()) {
        return Err(CliError::Config(
            "--instrumental cannot be combined with --lyrics or --lyrics-file; use --instrumental alone for an unconstrained instrumental, or omit it and use bracketed [Instrumental] structure through --lyrics/--lyrics-file"
                .into(),
        ));
    }
    if args.lyrics_project_id.is_some()
        && (args.instrumental || (args.lyrics.is_none() && args.lyrics_file.is_none()))
    {
        return Err(CliError::Config(
            "--lyrics-project-id requires explicit custom lyrics from --lyrics or --lyrics-file and cannot be used for description or unconstrained instrumental mode"
                .into(),
        ));
    }

    let lyrics = match (&args.lyrics, &args.lyrics_file) {
        (Some(l), _) => Some(l.clone()),
        (_, Some(path)) => Some(std::fs::read_to_string(path)?),
        _ => None,
    };
    let vocal = if args.instrumental {
        None
    } else {
        args.vocal.as_ref()
    };
    let tags = build_tags(args.tags.as_deref(), None);
    let control_sliders = build_control_sliders(args.weirdness, args.style_influence)?;

    let model = model_api_key(args.model.as_deref(), config)?;
    validate_requested_duration(args.duration)?;
    let mut req = GenerateRequest::new(&model, "custom");
    if let Some(lyrics) = lyrics {
        req.prompt = lyrics;
    }
    req.title = Some(args.title.clone().unwrap_or_default());
    req.tags = Some(tags.unwrap_or_default());
    req.negative_tags = args.exclude.clone().unwrap_or_default();
    req.duration = args.duration;
    req.make_instrumental = args.instrumental;
    req.persona_id = args.persona.clone();
    req.lyrics_project_id = args
        .lyrics_project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    if args.lyrics_project_id.is_some() && req.lyrics_project_id.is_none() {
        return Err(CliError::Config(
            "--lyrics-project-id must not be empty".into(),
        ));
    }
    req.metadata.control_sliders = control_sliders;
    req.metadata.vocal_gender = vocal.map(|gender| match gender {
        crate::cli::VocalGender::Male => "m".to_string(),
        crate::cli::VocalGender::Female => "f".to_string(),
    });
    if req.persona_id.is_some() {
        req.override_fields = vec!["prompt".to_string(), "tags".to_string()];
    }
    Ok(req)
}

pub(crate) async fn validate_lyrics_project_reference(
    req: &GenerateRequest,
    client: &SunoClient,
) -> Result<(), CliError> {
    let Some(project_id) = req.lyrics_project_id.as_deref() else {
        return Ok(());
    };
    client.lyrics_project(project_id).await?;
    Ok(())
}

const EMPTY_CLIP_ID: &str = "00000000-0000-0000-0000-000000000000";

async fn resolve_persona_reference(
    req: &mut GenerateRequest,
    client: &SunoClient,
) -> Result<(), CliError> {
    let Some(persona_id) = req.persona_id.clone() else {
        return Ok(());
    };
    let persona = client.get_persona(&persona_id).await?;
    let root_clip_id = usable_persona_root_clip_id(persona.root_clip_id.as_deref());
    let root_duration = if let Some(root_clip_id) = root_clip_id.as_deref() {
        if let Some(duration) = persona
            .clip
            .as_ref()
            .filter(|clip| clip.id == root_clip_id)
            .and_then(|clip| clip.metadata.duration)
            .filter(|duration| duration.is_finite() && *duration >= 0.0)
        {
            Some(duration)
        } else {
            client
                .get_clip(root_clip_id)
                .await?
                .and_then(|clip| clip.metadata.duration)
                .filter(|duration| duration.is_finite() && *duration >= 0.0)
        }
    } else {
        None
    };
    apply_persona_reference(req, &persona_id, persona, root_duration)
}

fn usable_persona_root_clip_id(root_clip_id: Option<&str>) -> Option<String> {
    root_clip_id.and_then(|clip_id| {
        let clip_id = clip_id.trim();
        (!clip_id.is_empty() && clip_id != EMPTY_CLIP_ID).then(|| clip_id.to_string())
    })
}

fn apply_persona_reference(
    req: &mut GenerateRequest,
    requested_persona_id: &str,
    persona: crate::api::types::PersonaInfo,
    root_duration: Option<f64>,
) -> Result<(), CliError> {
    if persona.id != requested_persona_id {
        return Err(CliError::Api {
            code: "schema_drift",
            message: format!(
                "Suno returned Persona `{}` while resolving `{requested_persona_id}`",
                persona.id
            ),
        });
    }
    if persona.is_trashed || persona.is_hidden {
        return Err(CliError::Config(format!(
            "Persona `{requested_persona_id}` is trashed or hidden and cannot be used for generation"
        )));
    }

    let is_vox = persona.is_vox_persona();
    let root_clip_id = usable_persona_root_clip_id(persona.root_clip_id.as_deref());
    if !is_vox && root_clip_id.is_none() {
        return Err(CliError::Config(format!(
            "Persona `{requested_persona_id}` has no usable root clip for the current artist_consistency protocol"
        )));
    }

    req.task = Some(if is_vox {
        "vox".to_string()
    } else {
        "artist_consistency".to_string()
    });
    req.artist_clip_id = root_clip_id;
    req.artist_start_s = Some(0.0);
    req.artist_end_s = req.artist_clip_id.as_ref().and(root_duration);
    Ok(())
}

async fn describe(args: DescribeArgs, ctx: &AppContext) -> Result<(), CliError> {
    let mut req = build_describe_request(&args, &ctx.config)?;
    let challenge_mode = ChallengeMode::from_flags(args.captcha, args.no_captcha);
    let token = args.token.clone();
    let should_enhance_tags = args.enhance_tags;
    let instrumental = args.instrumental;

    if !ctx.quiet {
        eprintln!(
            "Submitting description ({})...",
            model_label(args.model.as_ref(), &ctx.config)
        );
    }
    let clips = execute_generation_submission(token, challenge_mode, ctx, move || async move {
        let client = ctx.client().await?;
        resolve_persona_reference(&mut req, &client).await?;
        if should_enhance_tags {
            let limits = client
                .prepare_generation_request_with_features(&mut req, &[TAG_UPSAMPLE_FEATURE])
                .await?;
            enhance_tags(&mut req, instrumental, &client).await?;
            validate_generation_lengths_with_limits(&req, &limits)?;
        } else {
            client.prepare_generation_request(&mut req).await?;
        }
        Ok((client, req))
    })
    .await?;
    output_generation(&clips, ctx);
    Ok(())
}

fn build_describe_request(
    args: &DescribeArgs,
    config: &AppConfig,
) -> Result<GenerateRequest, CliError> {
    let tags = build_tags(args.tags.as_deref(), args.vocal.as_ref());
    let control_sliders = build_control_sliders(args.weirdness, args.style_influence)?;

    let model = model_api_key(args.model.as_deref(), config)?;
    validate_requested_duration(args.duration)?;
    let mut req = GenerateRequest::new(&model, "simple");
    req.gpt_description_prompt = Some(args.prompt.clone());
    req.metadata.lyrics_model = Some("default".into());
    req.title = args.title.clone();
    req.tags = tags;
    req.negative_tags = args.exclude.clone().unwrap_or_default();
    req.duration = args.duration;
    req.make_instrumental = args.instrumental;
    req.persona_id = args.persona.clone();
    req.metadata.control_sliders = control_sliders;
    if req.tags.is_some() {
        mark_tags_override(&mut req);
    }
    if req.persona_id.is_some() {
        req.override_fields = vec!["prompt".to_string(), "tags".to_string()];
    }
    Ok(req)
}

async fn enhance_tags(
    req: &mut GenerateRequest,
    is_instrumental: bool,
    client: &SunoClient,
) -> Result<(), CliError> {
    let personalization_enabled = client.styles_augmentation_enabled().await?;
    let original_tags = req.tags.clone().unwrap_or_default();
    let lyrics = (!is_instrumental)
        .then_some(req.prompt.trim())
        .filter(|lyrics| !lyrics.is_empty());
    let upsample = client
        .upsample_tags(crate::api::types::PromptUpsampleRequest {
            original_tags: &original_tags,
            lyrics,
            is_instrumental,
            user_guidance: None,
        })
        .await?;
    req.tags = Some(upsample.upsampled.clone());
    req.metadata.last_tags_generation = Some(LastTagsGeneration::from_upsample_response(
        original_tags,
        upsample,
        personalization_enabled,
    ));
    mark_tags_override(req);
    Ok(())
}

fn mark_tags_override(req: &mut GenerateRequest) {
    if !req.override_fields.iter().any(|field| field == "tags") {
        req.override_fields.push("tags".to_string());
    }
}

fn model_api_key(model: Option<&str>, config: &AppConfig) -> Result<String, CliError> {
    normalize_generation_model_selector(model.unwrap_or(config.default_model.as_str()))
}

fn model_label<'a>(model: Option<&'a String>, config: &'a AppConfig) -> &'a str {
    model.map(String::as_str).unwrap_or_else(|| {
        if config.default_model == "auto" {
            "account default"
        } else {
            config.default_model.as_str()
        }
    })
}

fn validate_requested_duration(duration: Option<f64>) -> Result<(), CliError> {
    if let Some(duration) = duration
        && (!duration.is_finite() || duration <= 0.0)
    {
        return Err(CliError::Config(
            "--duration must be a positive finite number of seconds".into(),
        ));
    }
    Ok(())
}

pub async fn extend(args: ExtendArgs, ctx: &AppContext) -> Result<(), CliError> {
    crate::core::ensure_non_negative_finite("--at", args.at)?;
    let challenge_mode = ChallengeMode::from_flags(args.captcha, args.no_captcha);
    let token = args.token.clone();
    let instrumental = if args.instrumental {
        Some(true)
    } else if args.no_instrumental {
        Some(false)
    } else {
        None
    };
    let clips = execute_generation_submission(token, challenge_mode, ctx, move || async move {
        let client = ctx.client().await?;
        let mut req = client
            .prepare_extend_request(ExtendClipOptions {
                clip_id: &args.clip_id,
                continue_at: args.at,
                tags: args.tags.as_deref(),
                negative_tags: args.exclude.as_deref(),
                lyrics: args.lyrics.as_deref(),
                title: args.title.as_deref(),
                instrumental,
                challenge_token: None,
                model: ctx.config.default_model.as_str(),
            })
            .await?;
        client.prepare_generation_request(&mut req).await?;
        Ok((client, req))
    })
    .await?;
    output_generation(&clips, ctx);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::api::types::{GenerateRequest, PersonaInfo};
    use crate::cli::{CreateArgs, DescribeArgs};
    use crate::core::AppConfig;

    use super::{
        apply_persona_reference, build_describe_args_from_create, build_describe_request,
        build_generate_args_from_create, build_generate_request, mark_tags_override,
        validate_create_lyrics_project_mode,
    };

    fn persona_fixture(id: &str, persona_type: &str, root_clip_id: Option<&str>) -> PersonaInfo {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": "Protocol Persona",
            "persona_type": persona_type,
            "root_clip_id": root_clip_id
        }))
        .expect("persona fixture")
    }

    fn config_with_default_model(default_model: &str) -> AppConfig {
        AppConfig {
            default_model: default_model.to_string(),
            ..AppConfig::default()
        }
    }

    fn describe_args(title: Option<String>, model: Option<String>) -> DescribeArgs {
        DescribeArgs {
            title,
            prompt: "bright city pop about a clean morning".into(),
            tags: Some("city pop, bright".into()),
            exclude: None,
            model,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        }
    }

    #[test]
    fn describe_request_omits_title_by_default() {
        let config = AppConfig::default();

        let req = build_describe_request(&describe_args(None, Some("v5.5".into())), &config)
            .expect("request");

        let body = serde_json::to_value(req).expect("request json");
        assert!(
            !body
                .as_object()
                .expect("request object")
                .contains_key("title")
        );
        assert_eq!(body["metadata"]["create_mode"], "simple");
        assert_eq!(
            body["gpt_description_prompt"],
            "bright city pop about a clean morning"
        );
        assert_eq!(body["prompt"], "");
        assert_eq!(body["metadata"]["lyrics_model"], "default");
        assert_eq!(body["override_fields"], serde_json::json!(["tags"]));
    }

    #[test]
    fn describe_request_uses_supplied_title() {
        let config = AppConfig::default();

        let req = build_describe_request(
            &describe_args(Some("Morning Reset".into()), Some("v5.5".into())),
            &config,
        )
        .expect("request");

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["title"], "Morning Reset");
    }

    #[test]
    fn describe_request_preserves_known_display_selector_and_writes_duration() {
        let mut args = describe_args(None, Some("V5.5".into()));
        args.duration = Some(210.5);

        let request = build_describe_request(&args, &AppConfig::default()).expect("request");
        let body = serde_json::to_value(request).expect("request json");

        assert_eq!(body["mv"], "v5.5");
        assert_eq!(body["duration"], 210.5);
    }

    #[test]
    fn request_builder_rejects_non_positive_or_non_finite_duration() {
        for duration in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let mut args = describe_args(None, Some("v5.5".into()));
            args.duration = Some(duration);

            let error =
                build_describe_request(&args, &AppConfig::default()).expect_err("invalid duration");

            assert!(error.to_string().contains("positive finite"));
        }
    }

    #[test]
    fn describe_request_omits_unspecified_title_and_tags() {
        let config = AppConfig::default();
        let mut args = describe_args(None, Some("v5.5".into()));
        args.tags = None;

        let req = build_describe_request(&args, &config).expect("request");

        let body = serde_json::to_value(req).expect("request json");
        let object = body.as_object().expect("request object");
        assert!(!object.contains_key("title"));
        assert!(!object.contains_key("tags"));
        assert_eq!(body["override_fields"], serde_json::json!([]));
    }

    #[test]
    fn describe_request_uses_config_default_model_when_flag_is_omitted() {
        let config = config_with_default_model("chirp-crow");

        let req = build_describe_request(&describe_args(None, None), &config).expect("request");

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["mv"], "chirp-crow");
    }

    #[test]
    fn description_create_preserves_challenge_controls() {
        let args = CreateArgs {
            prompt: Some("a warm ballad about starlight".into()),
            title: Some("Starlight".into()),
            tags: Some("pop ballad".into()),
            exclude: None,
            lyrics: None,
            lyrics_file: None,
            lyrics_project_id: None,
            model: Some("v5.5".into()),
            duration: Some(222.0),
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: true,
            instrumental: false,
            token: Some("captcha-token".into()),
            captcha: true,
            no_captcha: false,
            persona: None,
        };

        let describe_args = build_describe_args_from_create(args).expect("describe args");

        assert_eq!(describe_args.token.as_deref(), Some("captcha-token"));
        assert!(describe_args.captcha);
        assert!(!describe_args.no_captcha);
        assert!(describe_args.enhance_tags);
        assert_eq!(describe_args.duration, Some(222.0));
    }

    #[test]
    fn description_create_preserves_excluded_styles() {
        let args = CreateArgs {
            prompt: Some("a warm ballad about starlight".into()),
            title: None,
            tags: None,
            exclude: Some("metal, spoken word".into()),
            lyrics: None,
            lyrics_file: None,
            lyrics_project_id: None,
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };

        let describe_args = build_describe_args_from_create(args).expect("describe args");
        let request = build_describe_request(&describe_args, &AppConfig::default())
            .expect("description request");

        assert_eq!(request.negative_tags, "metal, spoken word");
    }

    #[test]
    fn generate_request_uses_config_default_model_when_flag_is_omitted() {
        let args = crate::cli::GenerateArgs {
            title: Some("Morning Reset".into()),
            tags: Some("city pop".into()),
            exclude: None,
            lyrics: Some("[Verse]\nHello".into()),
            lyrics_file: None,
            lyrics_project_id: None,
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };
        let config = config_with_default_model("chirp-crow");

        let req = build_generate_request(&args, &config).expect("request");

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["mv"], "chirp-crow");
        assert_eq!(body["prompt"], "[Verse]\nHello");
        assert!(
            !body
                .as_object()
                .expect("object")
                .contains_key("gpt_description_prompt")
        );
        assert!(
            !body["metadata"]
                .as_object()
                .expect("metadata object")
                .contains_key("lyrics_model")
        );
        assert!(body["token"].is_null());
        assert!(body["token_provider"].is_null());
    }

    #[test]
    fn custom_request_transmits_the_validated_lyrics_project_id_exactly() {
        let args = crate::cli::GenerateArgs {
            title: Some("Project Draft".into()),
            tags: Some("indie pop".into()),
            exclude: None,
            lyrics: Some("[Verse]\nhello".into()),
            lyrics_file: None,
            lyrics_project_id: Some("project-1".into()),
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };

        let request = build_generate_request(&args, &AppConfig::default()).expect("request");
        let body = serde_json::to_value(request).expect("request JSON");
        assert_eq!(body["prompt"], "[Verse]\nhello");
        assert_eq!(body["lyrics_project_id"], "project-1");
        assert_eq!(body["metadata"]["create_mode"], "custom");
    }

    #[test]
    fn lyrics_project_id_fails_closed_for_description_or_instrumental_mode() {
        let description = CreateArgs {
            prompt: Some("describe a song".into()),
            title: None,
            tags: None,
            exclude: None,
            lyrics: None,
            lyrics_file: None,
            lyrics_project_id: Some("project-1".into()),
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };
        let error = validate_create_lyrics_project_mode(&description)
            .expect_err("description mode must reject project ID");
        assert!(
            error
                .to_string()
                .contains("requires explicit custom lyrics")
        );

        let instrumental = CreateArgs {
            instrumental: true,
            ..description
        };
        validate_create_lyrics_project_mode(&instrumental)
            .expect_err("unconstrained instrumental mode must reject project ID");
    }

    #[test]
    fn custom_request_uses_current_web_vocal_gender_field() {
        let args = crate::cli::GenerateArgs {
            title: Some("Morning Reset".into()),
            tags: Some("city pop".into()),
            exclude: None,
            lyrics: Some("[Verse]\nHello".into()),
            lyrics_file: None,
            lyrics_project_id: None,
            model: None,
            duration: Some(180.0),
            vocal: Some(crate::cli::VocalGender::Female),
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };

        let req = build_generate_request(&args, &AppConfig::default()).expect("request");
        let body = serde_json::to_value(req).expect("request json");

        assert_eq!(body["tags"], "city pop");
        assert_eq!(body["metadata"]["vocal_gender"], "f");
        assert_eq!(body["duration"], 180.0);
    }

    #[test]
    fn custom_request_sends_web_empty_strings_and_persona_overrides() {
        let args = crate::cli::GenerateArgs {
            title: None,
            tags: None,
            exclude: None,
            lyrics: Some("[Verse]\nHello".into()),
            lyrics_file: None,
            lyrics_project_id: None,
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: Some("persona-1".into()),
        };

        let req = build_generate_request(&args, &AppConfig::default()).expect("request");
        let body = serde_json::to_value(req).expect("request json");

        assert_eq!(body["title"], "");
        assert_eq!(body["tags"], "");
        assert_eq!(
            body["override_fields"],
            serde_json::json!(["prompt", "tags"])
        );
    }

    #[test]
    fn vox_persona_reference_uses_current_rootless_advanced_contract() {
        let mut req = GenerateRequest::new("auto", "custom");
        req.persona_id = Some("persona-vox".into());

        apply_persona_reference(
            &mut req,
            "persona-vox",
            persona_fixture(
                "persona-vox",
                "vox",
                Some("00000000-0000-0000-0000-000000000000"),
            ),
            None,
        )
        .expect("rootless Vox reference");

        assert_eq!(req.task.as_deref(), Some("vox"));
        assert_eq!(req.persona_id.as_deref(), Some("persona-vox"));
        assert!(req.artist_clip_id.is_none());
        assert_eq!(req.artist_start_s, Some(0.0));
        assert!(req.artist_end_s.is_none());
    }

    #[test]
    fn flattened_vox_flag_uses_current_rootless_advanced_contract() {
        let mut req = GenerateRequest::new("auto", "custom");
        req.persona_id = Some("persona-vox".into());
        let persona: PersonaInfo = serde_json::from_value(serde_json::json!({
            "id": "persona-vox",
            "name": "Verified Voice",
            "is_vox_persona": true
        }))
        .expect("flattened Vox Persona fixture");

        apply_persona_reference(&mut req, "persona-vox", persona, None)
            .expect("rootless flattened Vox reference");

        assert_eq!(req.task.as_deref(), Some("vox"));
        assert_eq!(req.persona_id.as_deref(), Some("persona-vox"));
        assert!(req.artist_clip_id.is_none());
        assert_eq!(req.artist_start_s, Some(0.0));
        assert!(req.artist_end_s.is_none());
    }

    #[test]
    fn legacy_persona_reference_uses_current_artist_consistency_contract() {
        let mut req = GenerateRequest::new("auto", "simple");
        req.persona_id = Some("persona-legacy".into());

        apply_persona_reference(
            &mut req,
            "persona-legacy",
            persona_fixture("persona-legacy", "legacy", Some("clip-root")),
            Some(239.84),
        )
        .expect("legacy Persona reference");

        assert_eq!(req.task.as_deref(), Some("artist_consistency"));
        assert_eq!(req.artist_clip_id.as_deref(), Some("clip-root"));
        assert_eq!(req.artist_start_s, Some(0.0));
        assert_eq!(req.artist_end_s, Some(239.84));
    }

    #[test]
    fn legacy_persona_without_a_root_clip_fails_closed() {
        let mut req = GenerateRequest::new("auto", "custom");
        req.persona_id = Some("persona-legacy".into());

        let error = apply_persona_reference(
            &mut req,
            "persona-legacy",
            persona_fixture("persona-legacy", "legacy", None),
            None,
        )
        .expect_err("rootless legacy Persona must not submit");

        assert!(error.to_string().contains("no usable root clip"));
    }

    #[test]
    fn instrumental_generate_request_rejects_custom_lyrics() {
        let args = crate::cli::GenerateArgs {
            title: Some("Morning Reset".into()),
            tags: Some("city pop".into()),
            exclude: None,
            lyrics: Some("[Verse]\nHello".into()),
            lyrics_file: None,
            lyrics_project_id: None,
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: true,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };
        let config = config_with_default_model("chirp-crow");

        let error = build_generate_request(&args, &config).expect_err("conflicting inputs");

        assert!(
            error
                .to_string()
                .contains("--instrumental cannot be combined with --lyrics or --lyrics-file")
        );
    }

    #[test]
    fn bracketed_instrumental_structure_uses_custom_lyrics_contract() {
        let structure = "[Instrumental]\n[Intro — sparse felt piano]\n[Build — strings accelerate]";
        let args = crate::cli::GenerateArgs {
            title: Some("Structured Score".into()),
            tags: Some("cinematic orchestral".into()),
            exclude: Some("vocals, spoken word".into()),
            lyrics: Some(structure.into()),
            lyrics_file: None,
            lyrics_project_id: None,
            model: None,
            duration: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            enhance_tags: false,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };

        let req = build_generate_request(&args, &AppConfig::default()).expect("request");
        let body = serde_json::to_value(req).expect("request json");

        assert_eq!(body["prompt"], structure);
        assert_eq!(body["make_instrumental"], false);
        assert_eq!(body["metadata"]["create_mode"], "custom");
        assert!(
            !body
                .as_object()
                .expect("object")
                .contains_key("gpt_description_prompt")
        );
    }

    #[test]
    fn instrumental_create_prompt_uses_custom_generation_contract() {
        let args = crate::cli::CreateArgs {
            prompt: Some("Full-length instrumental about heat before rain".into()),
            title: Some("Forty Degree Night Flight".into()),
            tags: Some("cinematic synth-rock, humid pads".into()),
            exclude: Some("vocal, spoken word".into()),
            lyrics: None,
            lyrics_file: None,
            lyrics_project_id: None,
            model: Some("v5.5".into()),
            duration: None,
            vocal: Some(crate::cli::VocalGender::Female),
            weirdness: Some(40.0),
            style_influence: Some(68.0),
            enhance_tags: true,
            instrumental: true,
            token: None,
            captcha: false,
            no_captcha: true,
            persona: None,
        };
        let config = AppConfig::default();

        let generate_args = build_generate_args_from_create(args);
        assert!(generate_args.enhance_tags);
        let req = build_generate_request(&generate_args, &config).expect("request");

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["metadata"]["create_mode"], "custom");
        assert_eq!(body["prompt"], "");
        assert_eq!(body["make_instrumental"], true);
        assert_eq!(body["title"], "Forty Degree Night Flight");
        assert_eq!(
            body["tags"],
            "cinematic synth-rock, humid pads, Full-length instrumental about heat before rain"
        );
        assert_eq!(body["negative_tags"], "vocal, spoken word");
        assert_eq!(body["mv"], "v5.5");
        assert!(
            !body["tags"]
                .as_str()
                .expect("tags")
                .contains("female vocals")
        );
        assert!(
            !body
                .as_object()
                .expect("object")
                .contains_key("gpt_description_prompt")
        );
        assert!(
            !body["metadata"]
                .as_object()
                .expect("metadata object")
                .contains_key("lyrics_model")
        );
    }

    #[test]
    fn tag_upsample_marks_tags_override_once() {
        let mut req = crate::api::types::GenerateRequest::new("chirp-fenix", "custom");

        mark_tags_override(&mut req);
        mark_tags_override(&mut req);

        assert_eq!(req.override_fields, vec!["tags".to_string()]);
    }
}
