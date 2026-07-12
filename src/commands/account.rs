use crate::app::AppContext;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub async fn credits(ctx: &AppContext) -> Result<(), CliError> {
    let info = ctx.client().await?.billing_info().await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&info),
        OutputFormat::Table => output::table::billing(&info),
    }
    Ok(())
}

pub async fn models(ctx: &AppContext) -> Result<(), CliError> {
    let info = ctx.client().await?.billing_info().await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "generation": info.models,
            "remaster": info.remaster_model_types,
        })),
        OutputFormat::Table => {
            output::table::models(&info.models);
            output::table::remaster_models(&info.remaster_model_types);
        }
    }
    Ok(())
}
