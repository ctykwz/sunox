use std::future::Future;

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
    auth: &AuthState,
) -> Result<Option<String>, CliError> {
    if let Some(token) = token {
        return Ok(Some(token));
    }
    if force_captcha {
        if !ctx.quiet {
            eprintln!("Solving hCaptcha via piloted browser...");
        }
        return Ok(Some(captcha::solve(auth).await?));
    }

    Ok(None)
}

pub(super) async fn execute_generation_submission<Prepare, O, PrepareFuture, Submit, SubmitFuture>(
    token: Option<String>,
    force_captcha: bool,
    ctx: &AppContext,
    prepare: Prepare,
    submit: Submit,
) -> Result<O, CliError>
where
    Prepare: FnOnce() -> PrepareFuture,
    PrepareFuture: Future<
        Output = Result<(crate::api::SunoClient, crate::api::types::GenerateRequest), CliError>,
    >,
    Submit: FnOnce(
        (crate::api::SunoClient, crate::api::types::GenerateRequest),
        Option<String>,
    ) -> SubmitFuture,
    SubmitFuture: Future<Output = Result<O, CliError>>,
{
    let prepared = prepare().await?;
    let auth = prepared.0.auth_state_snapshot();
    execute_prepared_submission_with(
        prepared,
        || ctx.acquire_mutation_lock_for(&auth),
        || generation_token(token, force_captcha, ctx, &auth),
        submit,
    )
    .await
}

async fn execute_prepared_submission_with<
    Guard,
    Prepared,
    O,
    Lock,
    Token,
    TokenFuture,
    Submit,
    SubmitFuture,
>(
    prepared: Prepared,
    acquire_lock: Lock,
    acquire_token: Token,
    submit: Submit,
) -> Result<O, CliError>
where
    Lock: FnOnce() -> Result<Guard, CliError>,
    Token: FnOnce() -> TokenFuture,
    TokenFuture: Future<Output = Result<Option<String>, CliError>>,
    Submit: FnOnce(Prepared, Option<String>) -> SubmitFuture,
    SubmitFuture: Future<Output = Result<O, CliError>>,
{
    let _guard = acquire_lock()?;
    let token = acquire_token().await?;
    submit(prepared, token).await
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::execute_prepared_submission_with;

    #[tokio::test]
    async fn generation_submission_solves_challenge_after_preparation_and_before_submit() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let lock_events = Arc::clone(&events);
        let token_events = Arc::clone(&events);
        let submit_events = Arc::clone(&events);

        events.lock().expect("events mutex").push("prepare");
        let result = execute_prepared_submission_with(
            "prepared-request",
            move || {
                lock_events.lock().expect("events mutex").push("lock");
                Ok(())
            },
            move || async move {
                token_events.lock().expect("events mutex").push("token");
                Ok(Some("challenge-token".to_string()))
            },
            move |prepared, token| async move {
                submit_events.lock().expect("events mutex").push("submit");
                assert_eq!(prepared, "prepared-request");
                assert_eq!(token.as_deref(), Some("challenge-token"));
                Ok("submitted")
            },
        )
        .await
        .expect("generation submission");

        assert_eq!(result, "submitted");
        assert_eq!(
            *events.lock().expect("events mutex"),
            ["prepare", "lock", "token", "submit"]
        );
    }
}
