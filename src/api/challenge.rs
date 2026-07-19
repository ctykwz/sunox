use serde::{Deserialize, Serialize};

use super::SunoClient;
use crate::core::CliError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeProvider {
    HCaptcha = 1,
    Turnstile = 2,
}

impl ChallengeProvider {
    pub const fn token_provider(self) -> u8 {
        self as u8
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::HCaptcha => "hCaptcha",
            Self::Turnstile => "Cloudflare Turnstile",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct GenerationChallenge {
    #[serde(default)]
    pub required: bool,
    pub captcha_version: Option<u8>,
}

impl GenerationChallenge {
    /// The web client currently normalizes every generation captcha version
    /// other than version 2 to the hCaptcha (provider 1) flow.
    pub fn provider(self) -> ChallengeProvider {
        if self.captcha_version == Some(2) {
            ChallengeProvider::Turnstile
        } else {
            ChallengeProvider::HCaptcha
        }
    }
}

#[derive(Serialize)]
struct ChallengeCheckRequest<'a> {
    ctype: &'a str,
}

impl SunoClient {
    /// Check the same generation challenge gate the web client calls before
    /// submitting `/api/generate/v2-web/`.
    pub async fn generation_challenge(&self) -> Result<GenerationChallenge, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/c/check")
                .json(&ChallengeCheckRequest {
                    ctype: "generation",
                })
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Match the web session's retry behavior closely enough for CLI use: if
    /// the challenge gate is active, refresh a reusable Clerk session once and
    /// repeat the preflight before deciding whether a browser token is needed.
    pub(crate) async fn generation_challenge_with_refresh(
        &self,
    ) -> Result<GenerationChallenge, CliError> {
        let mut challenge = self.generation_challenge().await?;
        if challenge.required && self.try_refresh_jwt_for_challenge_recheck().await? {
            challenge = self.generation_challenge().await?;
        }
        Ok(challenge)
    }
}

#[cfg(test)]
mod tests {
    use super::{ChallengeProvider, GenerationChallenge};

    #[test]
    fn challenge_provider_matches_current_web_normalization() {
        for version in [None, Some(0), Some(1), Some(3)] {
            assert_eq!(
                GenerationChallenge {
                    required: true,
                    captcha_version: version,
                }
                .provider(),
                ChallengeProvider::HCaptcha
            );
        }
        assert_eq!(
            GenerationChallenge {
                required: true,
                captcha_version: Some(2),
            }
            .provider(),
            ChallengeProvider::Turnstile
        );
    }
}
