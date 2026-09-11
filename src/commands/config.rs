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
        ConfigAction::Set { key, value } => set(&key, &value, ctx.fmt, &[]),
        ConfigAction::Check => {
            check(ctx).await?;
            Ok(())
        }
    }
}

/// Repair a single persisted field without requiring valid runtime settings.
pub fn set(
    key: &str,
    value: &str,
    format: OutputFormat,
    overrides: &[String],
) -> Result<(), CliError> {
    AppConfig::set_persisted(key, value).map_err(with_config_path)?;
    match AppConfig::load_with_overrides(overrides) {
        Ok(config) => match format {
            OutputFormat::Json => output::json::success(config),
            OutputFormat::Table => {
                eprintln!("Set {key}={value}");
                if let Some(path) = AppConfig::path() {
                    eprintln!("Config: {}", path.display());
                }
            }
        },
        Err(error) => {
            let path = AppConfig::path();
            match format {
                OutputFormat::Json => output::json::success(serde_json::json!({
                    "saved": true,
                    "key": key,
                    "value": value,
                    "path": path,
                    "effective_config": {
                        "ok": false,
                        "code": error.error_code(),
                        "message": error.to_string()
                    }
                })),
                OutputFormat::Table => {
                    eprintln!("Saved {key}={value}");
                    if let Some(path) = path {
                        eprintln!("Config: {}", path.display());
                    }
                    eprintln!("Effective configuration is still invalid: {error}");
                }
            }
        }
    }
    Ok(())
}

pub fn with_config_path(error: CliError) -> CliError {
    let path = AppConfig::path();
    CliError::Diagnostic {
        code: error.error_code(),
        message: match path.as_ref() {
            Some(path) => format!("configuration at {}: {error}", path.display()),
            None => format!("configuration: {error}"),
        },
        details: serde_json::json!({
            "config": {
                "ok": false,
                "path": path,
                "code": error.error_code(),
                "message": error.to_string()
            }
        }),
    }
}

async fn check(ctx: &AppContext) -> Result<(), CliError> {
    let result =
        match load_auth_state_with_recovered_environment(ctx.browser_launch_policy()?).await {
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
