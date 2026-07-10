use crate::app::AppContext;
use crate::cli::LyricsArgs;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn lyrics(args: LyricsArgs, ctx: &AppContext) -> Result<(), CliError> {
    if !ctx.quiet {
        eprintln!("Generating lyrics...");
    }
    let (client, _mutation_guard) = ctx.mutation_client().await?;
    let result = client.generate_lyrics(&args.prompt).await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&result),
        OutputFormat::Table => output::table::lyrics(&result),
    }
    Ok(())
}
