use crate::api::types::GenerateRequest;
use crate::app::AppContext;
use crate::cli::{CreateArgs, DescribeArgs, ExtendArgs, GenerateArgs, ModelVersion};
use crate::core::{AppConfig, CliError};
use crate::workflow::generation::{build_control_sliders, build_tags};

use super::support::{generation_token, output_clips};

pub async fn create(args: CreateArgs, ctx: &AppContext) -> Result<(), CliError> {
    if args.instrumental || args.lyrics.is_some() || args.lyrics_file.is_some() {
        return generate(build_generate_args_from_create(args), ctx).await;
    }

    describe(build_describe_args_from_create(args)?, ctx).await
}

fn build_describe_args_from_create(args: CreateArgs) -> Result<DescribeArgs, CliError> {
    let prompt = args
        .prompt
        .ok_or_else(|| CliError::Config("provide a prompt or --lyrics/--lyrics-file".into()))?;
    Ok(DescribeArgs {
        title: args.title,
        prompt,
        tags: args.tags,
        model: args.model,
        vocal: args.vocal,
        weirdness: args.weirdness,
        style_influence: args.style_influence,
        instrumental: args.instrumental,
        token: args.token,
        captcha: args.captcha,
        no_captcha: args.no_captcha,
        persona: args.persona,
    })
}

fn build_generate_args_from_create(args: CreateArgs) -> GenerateArgs {
    let tags = if args.instrumental {
        merge_instrumental_prompt_and_tags(args.prompt, args.tags)
    } else {
        args.tags
    };

    GenerateArgs {
        title: args.title,
        tags,
        exclude: args.exclude,
        lyrics: if args.instrumental { None } else { args.lyrics },
        lyrics_file: if args.instrumental {
            None
        } else {
            args.lyrics_file
        },
        model: args.model,
        vocal: if args.instrumental { None } else { args.vocal },
        weirdness: args.weirdness,
        style_influence: args.style_influence,
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
    let force_captcha = args.captcha && !args.no_captcha;
    let client = ctx.client().await?;
    req.set_challenge_token(generation_token(args.token.clone(), force_captcha, ctx).await?);

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
    let clips = client.generate(&req).await?;
    output_clips(&clips, ctx);
    Ok(())
}

fn build_generate_request(
    args: &GenerateArgs,
    config: &AppConfig,
) -> Result<GenerateRequest, CliError> {
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
    let tags = build_tags(args.tags.as_deref(), vocal);
    let control_sliders = build_control_sliders(args.weirdness, args.style_influence);

    let mut req = GenerateRequest::new(model_api_key(args.model.as_ref(), config), "custom");
    if let (Some(lyrics), false) = (lyrics, args.instrumental) {
        req.gpt_description_prompt = Some(lyrics);
        req.metadata.lyrics_model = Some("default".into());
    }
    req.title = args.title.clone();
    req.tags = tags;
    req.negative_tags = args.exclude.clone().unwrap_or_default();
    req.make_instrumental = args.instrumental;
    req.persona_id = args.persona.clone();
    req.metadata.control_sliders = control_sliders;
    Ok(req)
}

async fn describe(args: DescribeArgs, ctx: &AppContext) -> Result<(), CliError> {
    let mut req = build_describe_request(&args, &ctx.config);
    let force_captcha = args.captcha && !args.no_captcha;
    let client = ctx.client().await?;
    req.set_challenge_token(generation_token(args.token.clone(), force_captcha, ctx).await?);

    if !ctx.quiet {
        eprintln!(
            "Submitting description ({})...",
            model_label(args.model.as_ref(), &ctx.config)
        );
    }
    let clips = client.generate(&req).await?;
    output_clips(&clips, ctx);
    Ok(())
}

fn build_describe_request(args: &DescribeArgs, config: &AppConfig) -> GenerateRequest {
    let tags = build_tags(args.tags.as_deref(), args.vocal.as_ref());
    let control_sliders = build_control_sliders(args.weirdness, args.style_influence);

    let mut req = GenerateRequest::new(model_api_key(args.model.as_ref(), config), "inspiration");
    req.prompt = args.prompt.clone();
    req.title = Some(args.title.clone().unwrap_or_default());
    req.tags = tags;
    req.make_instrumental = args.instrumental;
    req.persona_id = args.persona.clone();
    req.metadata.control_sliders = control_sliders;
    req
}

fn model_api_key<'a>(model: Option<&'a ModelVersion>, config: &'a AppConfig) -> &'a str {
    model
        .map(ModelVersion::to_api_key)
        .unwrap_or(config.default_model.as_str())
}

fn model_label<'a>(model: Option<&'a ModelVersion>, config: &'a AppConfig) -> &'a str {
    model
        .map(ModelVersion::display_name)
        .unwrap_or(config.default_model.as_str())
}

pub async fn extend(args: ExtendArgs, ctx: &AppContext) -> Result<(), CliError> {
    let mut req = GenerateRequest::new("chirp-fenix", "custom");
    req.prompt = args.lyrics.unwrap_or_default();
    req.tags = args.tags;
    req.continue_clip_id = Some(args.clip_id);
    req.continue_at = Some(args.at);
    let force_captcha = args.captcha && !args.no_captcha;
    req.set_challenge_token(generation_token(args.token.clone(), force_captcha, ctx).await?);

    let client = ctx.client().await?;
    let clips = client.generate(&req).await?;
    output_clips(&clips, ctx);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::cli::{CreateArgs, DescribeArgs, ModelVersion};
    use crate::core::AppConfig;

    use super::{
        build_describe_args_from_create, build_describe_request, build_generate_args_from_create,
        build_generate_request,
    };

    fn config_with_default_model(default_model: &str) -> AppConfig {
        AppConfig {
            default_model: default_model.to_string(),
            ..AppConfig::default()
        }
    }

    fn describe_args(title: Option<String>, model: Option<ModelVersion>) -> DescribeArgs {
        DescribeArgs {
            title,
            prompt: "bright city pop about a clean morning".into(),
            tags: Some("city pop, bright".into()),
            model,
            vocal: None,
            weirdness: None,
            style_influence: None,
            instrumental: false,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        }
    }

    #[test]
    fn describe_request_sends_empty_title_by_default() {
        let config = AppConfig::default();

        let req = build_describe_request(&describe_args(None, Some(ModelVersion::V55)), &config);

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["title"], "");
        assert_eq!(body["metadata"]["create_mode"], "inspiration");
    }

    #[test]
    fn describe_request_uses_supplied_title() {
        let config = AppConfig::default();

        let req = build_describe_request(
            &describe_args(Some("Morning Reset".into()), Some(ModelVersion::V55)),
            &config,
        );

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["title"], "Morning Reset");
    }

    #[test]
    fn describe_request_uses_config_default_model_when_flag_is_omitted() {
        let config = config_with_default_model("chirp-crow");

        let req = build_describe_request(&describe_args(None, None), &config);

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
            model: Some(ModelVersion::V55),
            vocal: None,
            weirdness: None,
            style_influence: None,
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
    }

    #[test]
    fn generate_request_uses_config_default_model_when_flag_is_omitted() {
        let args = crate::cli::GenerateArgs {
            title: Some("Morning Reset".into()),
            tags: Some("city pop".into()),
            exclude: None,
            lyrics: Some("[Verse]\nHello".into()),
            lyrics_file: None,
            model: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
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
        assert_eq!(body["prompt"], "");
        assert_eq!(body["gpt_description_prompt"], "[Verse]\nHello");
        assert_eq!(body["metadata"]["lyrics_model"], "default");
        assert!(
            body.as_object()
                .expect("object")
                .contains_key("token_provider")
        );
        assert!(body["token_provider"].is_null());
    }

    #[test]
    fn instrumental_generate_request_omits_custom_lyrics_fields() {
        let args = crate::cli::GenerateArgs {
            title: Some("Morning Reset".into()),
            tags: Some("city pop".into()),
            exclude: None,
            lyrics: Some("[Verse]\nHello".into()),
            lyrics_file: None,
            model: None,
            vocal: None,
            weirdness: None,
            style_influence: None,
            instrumental: true,
            token: None,
            captcha: false,
            no_captcha: false,
            persona: None,
        };
        let config = config_with_default_model("chirp-crow");

        let req = build_generate_request(&args, &config).expect("request");

        let body = serde_json::to_value(req).expect("request json");
        assert_eq!(body["prompt"], "");
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
    fn instrumental_create_prompt_uses_custom_generation_contract() {
        let args = crate::cli::CreateArgs {
            prompt: Some("Full-length instrumental about heat before rain".into()),
            title: Some("Forty Degree Night Flight".into()),
            tags: Some("cinematic synth-rock, humid pads".into()),
            exclude: Some("vocal, spoken word".into()),
            lyrics: None,
            lyrics_file: None,
            model: Some(ModelVersion::V55),
            vocal: Some(crate::cli::VocalGender::Female),
            weirdness: Some(40.0),
            style_influence: Some(68.0),
            instrumental: true,
            token: None,
            captcha: false,
            no_captcha: true,
            persona: None,
        };
        let config = AppConfig::default();

        let generate_args = build_generate_args_from_create(args);
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
        assert_eq!(body["mv"], "chirp-fenix");
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
}
