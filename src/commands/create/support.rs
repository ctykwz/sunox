use std::future::Future;

use crate::api::challenge::{ChallengeProvider, GenerationChallenge};
use crate::api::types::Clip;
use crate::app::AppContext;
use crate::captcha;
use crate::core::CliError;
use crate::output::{self, OutputFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ChallengeMode {
    Auto,
    Force,
    Disabled,
}

impl ChallengeMode {
    pub(super) fn from_flags(force: bool, disabled: bool) -> Self {
        if force {
            Self::Force
        } else if disabled {
            Self::Disabled
        } else {
            Self::Auto
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ChallengeSolution {
    token: String,
    provider: ChallengeProvider,
}

async fn resolve_generation_challenge<Check, CheckFuture, Solve, SolveFuture>(
    explicit_token: Option<String>,
    mode: ChallengeMode,
    check: Check,
    solve: Solve,
) -> Result<Option<ChallengeSolution>, CliError>
where
    Check: FnOnce() -> CheckFuture,
    CheckFuture: Future<Output = Result<GenerationChallenge, CliError>>,
    Solve: FnOnce(ChallengeProvider) -> SolveFuture,
    SolveFuture: Future<Output = Result<String, CliError>>,
{
    let challenge = match check().await {
        Ok(challenge) => challenge,
        Err(error) if explicit_token.is_some() && !error.is_auth_or_rate_limit() => {
            GenerationChallenge {
                required: true,
                captcha_version: Some(1),
            }
        }
        Err(error) => return Err(error),
    };
    let provider = challenge.provider();

    if let Some(token) = explicit_token {
        return Ok(Some(ChallengeSolution { token, provider }));
    }

    if !challenge.required && mode != ChallengeMode::Force {
        return Ok(None);
    }
    if mode == ChallengeMode::Disabled {
        return Err(challenge_required_error(&challenge, None));
    }

    let token = solve(provider).await.map_err(|error| {
        challenge_required_error(
            &challenge,
            Some(format!(
                "Automatic {} verification failed: {error}",
                provider.label()
            )),
        )
    })?;
    Ok(Some(ChallengeSolution { token, provider }))
}

pub(super) async fn execute_generation_submission<Prepare, PrepareFuture>(
    token: Option<String>,
    challenge_mode: ChallengeMode,
    ctx: &AppContext,
    prepare: Prepare,
) -> Result<Vec<Clip>, CliError>
where
    Prepare: FnOnce() -> PrepareFuture,
    PrepareFuture: Future<
        Output = Result<(crate::api::SunoClient, crate::api::types::GenerateRequest), CliError>,
    >,
{
    let (client, mut request) = prepare().await?;
    let initial_auth = client.auth_state_snapshot();
    let _guard = ctx.acquire_mutation_lock_for(&initial_auth)?;
    let solution = resolve_generation_challenge(
        token,
        challenge_mode,
        || client.generation_challenge_with_refresh(),
        |provider| {
            let client = &client;
            async move {
                if !ctx.quiet {
                    eprintln!(
                        "Suno requested {}; running the configured browser verification flow...",
                        provider.label()
                    );
                }
                // The preflight may have refreshed the JWT. Snapshot after it
                // so the browser receives the newest cookies and metadata.
                let refreshed_auth = client.auth_state_snapshot();
                captcha::solve(&refreshed_auth, provider, ctx.config.challenge_browser).await
            }
        },
    )
    .await?;
    if let Some(solution) = solution {
        request.set_challenge_token_with_provider(Some(solution.token), solution.provider);
    }
    client
        .submit_prepared_generation_after_challenge(&request)
        .await
}

fn challenge_required_error(challenge: &GenerationChallenge, detail: Option<String>) -> CliError {
    let version = challenge
        .captcha_version
        .map(|version| version.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let detail = detail
        .map(|detail| format!(" {detail}."))
        .unwrap_or_default();
    CliError::ChallengeRequired(format!(
        "Suno requires a generation challenge (captcha_version={version}).{detail} Keep a supported local Chrome, Edge, Brave, Arc, or Chromium installation available and ensure --no-captcha is not set, or provide a valid challenge token with --token <token>."
    ))
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
    use super::{ChallengeMode, resolve_generation_challenge};
    use crate::api::challenge::{ChallengeProvider, GenerationChallenge};
    use crate::core::CliError;

    #[tokio::test]
    async fn automatic_mode_solves_detected_turnstile_challenge() {
        let result = resolve_generation_challenge(
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(2),
                })
            },
            |provider| async move {
                assert_eq!(provider, ChallengeProvider::Turnstile);
                Ok("turnstile-token".to_string())
            },
        )
        .await
        .expect("challenge solution")
        .expect("token");

        assert_eq!(result.token, "turnstile-token");
        assert_eq!(result.provider, ChallengeProvider::Turnstile);
    }

    #[tokio::test]
    async fn automatic_mode_skips_solver_when_challenge_is_not_required() {
        let result = resolve_generation_challenge(
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: false,
                    captcha_version: None,
                })
            },
            |_| async { panic!("solver must not run") },
        )
        .await
        .expect("challenge decision");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn disabled_mode_surfaces_required_challenge_without_solver() {
        let error = resolve_generation_challenge(
            None,
            ChallengeMode::Disabled,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(1),
                })
            },
            |_| async { panic!("solver must not run") },
        )
        .await
        .expect_err("challenge must surface");

        assert!(matches!(error, CliError::ChallengeRequired(_)));
    }

    #[tokio::test]
    async fn explicit_token_uses_detected_provider_without_running_solver() {
        let result = resolve_generation_challenge(
            Some("external-token".to_string()),
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(2),
                })
            },
            |_| async { panic!("solver must not run") },
        )
        .await
        .expect("challenge solution")
        .expect("token");

        assert_eq!(result.token, "external-token");
        assert_eq!(result.provider, ChallengeProvider::Turnstile);
    }

    #[tokio::test]
    async fn force_mode_solves_even_when_preflight_does_not_require_a_challenge() {
        let result = resolve_generation_challenge(
            None,
            ChallengeMode::Force,
            || async {
                Ok(GenerationChallenge {
                    required: false,
                    captcha_version: None,
                })
            },
            |provider| async move {
                assert_eq!(provider, ChallengeProvider::HCaptcha);
                Ok("forced-token".to_string())
            },
        )
        .await
        .expect("challenge solution")
        .expect("token");

        assert_eq!(result.token, "forced-token");
    }

    #[tokio::test]
    async fn automatic_solver_failure_remains_a_challenge_error() {
        let error = resolve_generation_challenge(
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(1),
                })
            },
            |_| async { Err(CliError::Config("no supported browser".into())) },
        )
        .await
        .expect_err("solver failure");

        assert!(
            matches!(error, CliError::ChallengeRequired(message) if message.contains("no supported browser"))
        );
    }

    #[tokio::test]
    async fn explicit_token_falls_back_to_hcaptcha_when_optional_preflight_fails() {
        let result = resolve_generation_challenge(
            Some("external-token".to_string()),
            ChallengeMode::Auto,
            || async { Err(CliError::Config("preflight unavailable".into())) },
            |_| async { panic!("solver must not run") },
        )
        .await
        .expect("challenge solution")
        .expect("token");

        assert_eq!(result.provider, ChallengeProvider::HCaptcha);
    }

    #[test]
    fn challenge_flags_map_to_distinct_modes() {
        assert_eq!(ChallengeMode::from_flags(false, false), ChallengeMode::Auto);
        assert_eq!(ChallengeMode::from_flags(true, false), ChallengeMode::Force);
        assert_eq!(
            ChallengeMode::from_flags(false, true),
            ChallengeMode::Disabled
        );
    }
}
