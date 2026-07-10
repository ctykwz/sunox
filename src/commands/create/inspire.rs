use crate::api::inspiration::InspirationOptions;
use crate::app::AppContext;
use crate::cli::InspireArgs;
use crate::core::{CliError, ensure_percentage};

use super::support::{execute_generation_submission, output_clips};

pub async fn inspire(args: InspireArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_percentage("--weirdness", args.weirdness)?;
    let lyrics = match (args.lyrics, args.lyrics_file) {
        (Some(lyrics), _) => lyrics,
        (_, Some(path)) => std::fs::read_to_string(path)?,
        _ => {
            return Err(CliError::Config(
                "inspiration generation requires --lyrics or --lyrics-file".into(),
            ));
        }
    };
    let force_captcha = args.captcha && !args.no_captcha;
    let token = args.token;
    let negative_tags = args.exclude.unwrap_or_default();

    if !ctx.quiet {
        eprintln!("Generating from clip inspiration...");
    }
    let clips = execute_generation_submission(
        token,
        force_captcha,
        ctx,
        move || async move {
            let client = ctx.client().await?;
            let mut req = client
                .prepare_inspiration_request(InspirationOptions {
                    clip_id: &args.clip_id,
                    title: &args.title,
                    tags: &args.tags,
                    negative_tags: &negative_tags,
                    lyrics: &lyrics,
                    weirdness: args.weirdness,
                    challenge_token: None,
                })
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
