use crate::app::AppContext;
use crate::cli::WaitArgs;
use crate::core::{CliError, ensure_clip_ids, ensure_poll_timeout_secs};
use crate::output::{self, OutputFormat};
use crate::workflow::tasks;

pub async fn run(args: WaitArgs, ctx: &AppContext) -> Result<(), CliError> {
    ensure_clip_ids(&args.ids)?;
    let timeout_secs = args.timeout.unwrap_or(ctx.config.poll_timeout_secs);
    ensure_poll_timeout_secs(timeout_secs)?;

    let client = ctx.client().await?;
    if !ctx.quiet {
        eprintln!("Waiting for {} clip(s) to finish...", args.ids.len());
    }
    let clips = tasks::wait_for_clips(
        &client,
        &args.ids,
        timeout_secs,
        ctx.config.poll_interval_secs,
    )
    .await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&clips),
        OutputFormat::Table => output::table::clips(&clips),
    }
    Ok(())
}
