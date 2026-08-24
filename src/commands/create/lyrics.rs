use crate::api::lyrics::CowriteLyricsOptions;
use crate::app::AppContext;
use crate::cli::LyricsArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn lyrics(args: LyricsArgs, ctx: &AppContext) -> Result<(), CliError> {
    let prompt = args.prompt.as_deref().ok_or_else(|| {
        CliError::Config(
            "provide --prompt for Cowrite generation or use `sunox lyrics projects`".into(),
        )
    })?;
    if !ctx.quiet {
        eprintln!("Generating lyrics...");
    }
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = client
        .generate_lyrics(CowriteLyricsOptions {
            prompt,
            model: args.model.as_deref(),
            enable_thinking: args.thinking,
        })
        .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&result),
        OutputFormat::Table => output::table::lyrics(&result),
    }
    Ok(())
}
