use crate::api::types::Clip;
use crate::app::AppContext;
use crate::auth::AuthState;
use crate::captcha;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

pub(super) async fn generation_token(
    token: Option<String>,
    force_captcha: bool,
    ctx: &AppContext,
) -> Result<Option<String>, CliError> {
    if let Some(token) = token {
        return Ok(Some(token));
    }
    if force_captcha {
        if !ctx.quiet {
            eprintln!("Solving hCaptcha via piloted browser...");
        }
        let auth = AuthState::load()?;
        return Ok(Some(captcha::solve(&auth).await?));
    }

    Ok(None)
}

pub(super) fn output_clips(clips: &[Clip], ctx: &AppContext) {
    match ctx.fmt {
        OutputFormat::Json => output::json::success(clips),
        OutputFormat::Table => {
            output::table::clips(clips);
            if !clips.is_empty() {
                let ids = clips
                    .iter()
                    .map(|clip| clip.id.as_str())
                    .collect::<Vec<_>>()
                    .join(" ");
                eprintln!("\nUse `sunox clip wait {ids}` to wait for completion");
            }
        }
    }
}
