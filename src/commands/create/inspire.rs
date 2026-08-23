use crate::api::inspiration::InspirationOptions;
use crate::app::AppContext;
use crate::cli::InspireArgs;
use crate::core::{CliError, ensure_percentage};

use super::support::{ChallengeMode, execute_generation_submission, output_generation};

pub async fn inspire(args: InspireArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_percentage("--weirdness", args.weirdness)?;
    if let Some(audio_influence) = args.audio_influence {
        ensure_percentage("--audio-influence", audio_influence)?;
    }
    let lyrics = match (args.lyrics, args.lyrics_file) {
        (Some(lyrics), _) => lyrics,
        (_, Some(path)) => std::fs::read_to_string(path)?,
        _ => {
            return Err(CliError::Config(
                "inspiration generation requires --lyrics or --lyrics-file".into(),
            ));
        }
    };
    let challenge_mode = ChallengeMode::from_flags(args.captcha, args.no_captcha);
    let token = args.token;
    let negative_tags = args.exclude.unwrap_or_default();

    if !ctx.quiet {
        eprintln!("Generating from clip inspiration...");
    }
    let clips = execute_generation_submission(token, challenge_mode, ctx, move || async move {
        let client = ctx.client().await?;
        let req = client
            .prepare_inspiration_request(InspirationOptions {
                clip_id: &args.clip_id,
                title: &args.title,
                tags: &args.tags,
                enhance_tags: args.enhance_tags,
                negative_tags: &negative_tags,
                lyrics: &lyrics,
                weirdness: args.weirdness,
                audio_influence: args.audio_influence,
                challenge_token: None,
                model: ctx.config.default_model.as_str(),
            })
            .await?;
        Ok((client, req))
    })
    .await?;
    output_generation(&clips, ctx);
    Ok(())
}
