use std::future::Future;

use crate::api::challenge::{ChallengeProvider, GenerationChallenge};
use crate::api::types::{Clip, GenerationResult};
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

async fn resolve_generation_challenge<Check, CheckFuture, Solve, SolveFuture, Gate, GateFuture>(
    explicit_token: Option<String>,
    explicit_provider: Option<ChallengeProvider>,
    mode: ChallengeMode,
    check: Check,
    mut solve: Solve,
    fallback_allowed: Gate,
) -> Result<Option<ChallengeSolution>, CliError>
where
    Check: FnOnce() -> CheckFuture,
    CheckFuture: Future<Output = Result<GenerationChallenge, CliError>>,
    Solve: FnMut(ChallengeProvider) -> SolveFuture,
    SolveFuture: Future<Output = Result<String, CliError>>,
    Gate: FnOnce() -> GateFuture,
    GateFuture: Future<Output = Result<bool, CliError>>,
{
    validate_explicit_provider(&explicit_token, explicit_provider)?;
    let challenge = match check().await {
        Ok(challenge) => challenge,
        Err(error) if explicit_token.is_some() && !error.is_auth_or_rate_limit() => {
            if explicit_provider.is_none() {
                return Err(CliError::Config("Challenge preflight is unavailable; supply --token-provider hcaptcha|turnstile with --token so its provider is not guessed".into()));
            }
            GenerationChallenge {
                required: true,
                captcha_version: Some(1),
            }
        }
        Err(error) => return Err(error),
    };
    let provider = challenge.provider();

    if let Some(token) = explicit_token {
        if explicit_provider.is_none() && provider == ChallengeProvider::Turnstile {
            return Err(CliError::Config("The web client can switch from Turnstile to hCaptcha; supply --token-provider hcaptcha|turnstile for this external --token".into()));
        }
        return Ok(Some(ChallengeSolution {
            token,
            provider: explicit_provider.unwrap_or(provider),
        }));
    }
    if !challenge.required && mode != ChallengeMode::Force {
        return Ok(None);
    }
    if mode == ChallengeMode::Disabled {
        return Err(challenge_required_error(&challenge, None));
    }

    let first = solve(provider).await;
    let (result, actual_provider) = match first {
        Err(error)
            if provider == ChallengeProvider::Turnstile
                && error.error_code() == "challenge_provider_failed" =>
        {
            match fallback_allowed().await {
                Ok(true) => (
                    solve(ChallengeProvider::HCaptcha).await,
                    ChallengeProvider::HCaptcha,
                ),
                Ok(false) => (Err(error), provider),
                Err(gate_error) => {
                    return Err(challenge_required_error(
                        &challenge,
                        Some(format!(
                            "Turnstile failed; could not verify Suno's hCaptcha fallback gate: {gate_error}"
                        )),
                    ));
                }
            }
        }
        other => (other, provider),
    };
    let token = result.map_err(|error| {
        challenge_required_error(
            &challenge,
            Some(format!(
                "Automatic {} verification failed: {error}",
                actual_provider.label()
            )),
        )
    })?;
    if token.trim().is_empty() {
        return Err(challenge_required_error(
            &challenge,
            Some("The verification provider returned an empty token".into()),
        ));
    }
    Ok(Some(ChallengeSolution {
        token,
        provider: actual_provider,
    }))
}

fn validate_explicit_provider(
    token: &Option<String>,
    provider: Option<ChallengeProvider>,
) -> Result<(), CliError> {
    if (provider.is_some() && token.is_none())
        || token.as_ref().is_some_and(|token| token.trim().is_empty())
    {
        return Err(CliError::Config(
            "--token-provider requires a nonempty --token".into(),
        ));
    }
    Ok(())
}

pub(super) async fn execute_generation_submission<Prepare, PrepareFuture>(
    token: Option<String>,
    token_provider: Option<ChallengeProvider>,
    challenge_mode: ChallengeMode,
    ctx: &AppContext,
    prepare: Prepare,
) -> Result<GenerationResult, CliError>
where
    Prepare: FnOnce(crate::api::SunoClient) -> PrepareFuture,
    PrepareFuture: Future<
        Output = Result<(crate::api::SunoClient, crate::api::types::GenerateRequest), CliError>,
    >,
{
    validate_explicit_provider(&token, token_provider)?;
    let (client, _guard) = ctx.mutation_client().await?;
    // Preparation can enhance tags through a remote POST. Keep that first
    // write under the same account guard as challenge resolution and submit,
    // with a validated timestamp so long preparation rechecks before writing.
    let (client, mut request) = prepare(client).await?;
    let solution = resolve_generation_challenge(
        token,
        token_provider,
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
        || async {
            client.try_refresh_jwt_for_challenge_recheck().await?;
            captcha::hcaptcha_fallback_allowed(&client.auth_state_snapshot()).await
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
        "Suno requires a generation challenge (captcha_version={version}).{detail} Keep a supported local Chrome, Edge, Brave, Arc, or Chromium installation available and ensure --no-captcha is not set, or provide a valid challenge token with --token <token> --token-provider <hcaptcha|turnstile>."
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

pub(super) fn output_generation(result: &GenerationResult, ctx: &AppContext) {
    match ctx.fmt {
        OutputFormat::Json => output::json::success(result),
        OutputFormat::Table => output_clips(&result.clips, ctx),
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
            || async { panic!("fallback gate must not run") },
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
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: false,
                    captcha_version: None,
                })
            },
            |_| async { panic!("solver must not run") },
            || async { panic!("fallback gate must not run") },
        )
        .await
        .expect("challenge decision");

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn disabled_mode_surfaces_required_challenge_without_solver() {
        let error = resolve_generation_challenge(
            None,
            None,
            ChallengeMode::Disabled,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(1),
                })
            },
            |_| async { panic!("solver must not run") },
            || async { panic!("fallback gate must not run") },
        )
        .await
        .expect_err("challenge must surface");

        assert!(matches!(error, CliError::ChallengeRequired(_)));
    }

    #[tokio::test]
    async fn external_token_without_provider_is_rejected_when_preflight_is_ambiguous() {
        let result = resolve_generation_challenge(
            Some("external-token".to_string()),
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(2),
                })
            },
            |_| async { panic!("solver must not run") },
            || async { panic!("gate must not run") },
        )
        .await;
        assert!(
            matches!(result, Err(CliError::Config(message)) if message.contains("--token-provider"))
        );
    }

    #[tokio::test]
    async fn force_mode_solves_even_when_preflight_does_not_require_a_challenge() {
        let result = resolve_generation_challenge(
            None,
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
            || async { panic!("fallback gate must not run") },
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
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(1),
                })
            },
            |_| async { Err(CliError::Config("no supported browser".into())) },
            || async { panic!("fallback gate must not run") },
        )
        .await
        .expect_err("solver failure");

        assert!(
            matches!(error, CliError::ChallengeRequired(message) if message.contains("no supported browser"))
        );
    }

    #[tokio::test]
    async fn explicit_provider_is_preserved_when_optional_preflight_fails() {
        let result = resolve_generation_challenge(
            Some("external-token".to_string()),
            Some(ChallengeProvider::HCaptcha),
            ChallengeMode::Auto,
            || async { Err(CliError::Config("preflight unavailable".into())) },
            |_| async { panic!("solver must not run") },
            || async { panic!("fallback gate must not run") },
        )
        .await
        .expect("challenge solution")
        .expect("token");

        assert_eq!(result.provider, ChallengeProvider::HCaptcha);
    }

    #[tokio::test]
    async fn web_fallback_updates_the_submitted_provider_and_runs_once() {
        let calls = std::cell::RefCell::new(Vec::new());
        let result = resolve_generation_challenge(
            None,
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(2),
                })
            },
            |provider| {
                calls.borrow_mut().push(provider);
                async move {
                    match provider {
                        ChallengeProvider::Turnstile => {
                            Err(crate::captcha::policy::provider_failure())
                        }
                        ChallengeProvider::HCaptcha => Ok("fresh-hcaptcha-token".into()),
                    }
                }
            },
            || async { Ok(true) },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            *calls.borrow(),
            vec![ChallengeProvider::Turnstile, ChallengeProvider::HCaptcha]
        );
        let mut request = crate::api::types::GenerateRequest::new("chirp-hawk", "custom");
        request.set_challenge_token_with_provider(Some(result.token), result.provider);
        let body = serde_json::to_value(request).unwrap();
        assert_eq!(body["token_provider"], 1);
        assert_eq!(body["token"], "fresh-hcaptcha-token");
    }

    #[tokio::test]
    async fn missing_or_disabled_gate_prevents_a_second_solver() {
        for gate in [Ok(false), Err(CliError::Config("unavailable seed".into()))] {
            let calls = std::cell::Cell::new(0);
            let result = resolve_generation_challenge(
                None,
                None,
                ChallengeMode::Auto,
                || async {
                    Ok(GenerationChallenge {
                        required: true,
                        captcha_version: Some(2),
                    })
                },
                |_| {
                    calls.set(calls.get() + 1);
                    async { Err(crate::captcha::policy::provider_failure()) }
                },
                || async { gate },
            )
            .await;
            assert!(result.is_err());
            assert_eq!(calls.get(), 1);
        }
    }

    #[tokio::test]
    async fn bridge_transport_auth_and_unknown_errors_never_trigger_provider_fallback() {
        for error in [
            CliError::AuthChanged,
            CliError::RateLimited,
            CliError::Config("bridge unavailable".into()),
            CliError::Api {
                code: "unknown_error",
                message: "unknown".into(),
            },
        ] {
            let mut error = Some(error);
            let result = resolve_generation_challenge(
                None,
                None,
                ChallengeMode::Auto,
                || async {
                    Ok(GenerationChallenge {
                        required: true,
                        captcha_version: Some(2),
                    })
                },
                |_| std::future::ready(Err(error.take().expect("only one attempt"))),
                || async { panic!("must not fetch fallback gate") },
            )
            .await;
            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn failed_hcaptcha_fallback_never_repeats_or_returns_a_submission_token() {
        let calls = std::cell::Cell::new(0);
        let result = resolve_generation_challenge(
            None,
            None,
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(2),
                })
            },
            |_| {
                calls.set(calls.get() + 1);
                async { Err(crate::captcha::policy::provider_failure()) }
            },
            || async { Ok(true) },
        )
        .await;
        assert!(matches!(result, Err(CliError::ChallengeRequired(_))));
        assert_eq!(calls.get(), 2);
    }

    #[tokio::test]
    async fn external_hcaptcha_provider_overrides_turnstile_preflight() {
        let result = resolve_generation_challenge(
            Some("external-hcaptcha-token".into()),
            Some(ChallengeProvider::HCaptcha),
            ChallengeMode::Auto,
            || async {
                Ok(GenerationChallenge {
                    required: true,
                    captcha_version: Some(2),
                })
            },
            |_| async { panic!("external token must not invoke solver") },
            || async { panic!("external token must not invoke gate") },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result.provider, ChallengeProvider::HCaptcha);
    }

    #[tokio::test]
    async fn external_provider_cannot_override_authentication_failure() {
        let result = resolve_generation_challenge(
            Some("external-token".into()),
            Some(ChallengeProvider::HCaptcha),
            ChallengeMode::Auto,
            || async { Err(CliError::AuthExpired) },
            |_| async { panic!("no solver") },
            || async { panic!("no gate") },
        )
        .await;
        assert!(matches!(result, Err(CliError::AuthExpired)));
    }

    #[tokio::test]
    async fn provider_without_token_fails_before_any_preflight() {
        let result = resolve_generation_challenge(
            None,
            Some(ChallengeProvider::HCaptcha),
            ChallengeMode::Auto,
            || async { panic!("no preflight") },
            |_| async { panic!("no solver") },
            || async { panic!("no gate") },
        )
        .await;
        assert!(matches!(result, Err(CliError::Config(_))));
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
