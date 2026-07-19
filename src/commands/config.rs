use crate::api::SunoClient;
use crate::app::AppContext;
use crate::auth::load_auth_state_with_recovered_environment;
use crate::cli::{ConfigAction, ConfigArgs};
use crate::core::{AppConfig, CliError};
use crate::output::{self, OutputFormat};

pub async fn run(args: ConfigArgs, ctx: &AppContext) -> Result<(), CliError> {
    match args.action {
        ConfigAction::Show => {
            match ctx.fmt {
                OutputFormat::Json => output::json::success(&ctx.config),
                OutputFormat::Table => println!("{}", serde_json::to_string_pretty(&ctx.config)?),
            }
            Ok(())
        }
        ConfigAction::Set { key, value } => {
            let config = AppConfig::set_persisted(&key, &value)?;
            match ctx.fmt {
                OutputFormat::Json => output::json::success(config),
                OutputFormat::Table => {
                    eprintln!("Set {key}={value}");
                    if let Some(path) = AppConfig::path() {
                        eprintln!("Config: {}", path.display());
                    }
                }
            }
            Ok(())
        }
        ConfigAction::Check => {
            check(ctx).await?;
            Ok(())
        }
    }
}

async fn check(ctx: &AppContext) -> Result<(), CliError> {
    let result = match load_auth_state_with_recovered_environment().await {
        Ok(auth) => match SunoClient::new_with_refresh(auth).await {
            Ok(client) => {
                let info = client.billing_info().await?;
                if matches!(ctx.fmt, OutputFormat::Table) {
                    eprintln!(
                        "Auth: OK — {}, {} credits",
                        info.plan.name, info.total_credits_left
                    );
                }
                serde_json::json!({
                    "config": {
                        "ok": true,
                        "path": AppConfig::path().map(|path| path.display().to_string()),
                    },
                    "auth": {
                        "ok": true,
                        "plan": info.plan.name,
                        "credits": info.total_credits_left,
                    }
                })
            }
            Err(e) => {
                if !matches!(e, CliError::AuthExpired) {
                    return Err(e);
                }
                if matches!(ctx.fmt, OutputFormat::Table) {
                    eprintln!("Auth: expired — run `sunox login`");
                }
                serde_json::json!({
                    "config": {
                        "ok": true,
                        "path": AppConfig::path().map(|path| path.display().to_string()),
                    },
                    "auth": {
                        "ok": false,
                        "code": e.error_code(),
                        "message": e.to_string(),
                    }
                })
            }
        },
        Err(e @ CliError::AuthMissing) => {
            if matches!(ctx.fmt, OutputFormat::Table) {
                eprintln!("Auth: not configured — run `sunox login`");
            }
            serde_json::json!({
                "config": {
                    "ok": true,
                    "path": AppConfig::path().map(|path| path.display().to_string()),
                },
                "auth": {
                    "ok": false,
                    "code": e.error_code(),
                    "message": e.to_string(),
                }
            })
        }
        Err(e) => return Err(e),
    };

    if matches!(ctx.fmt, OutputFormat::Json) {
        output::json::success(result);
    }
    Ok(())
}
