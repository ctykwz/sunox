use crate::api::SunoClient;
use crate::auth::{AuthState, BrowserLaunchPolicy, load_auth_state_with_recovered_environment};
use crate::core::CliError;
use crate::core::{AppConfig, ChallengeBrowserMode};
use crate::output::OutputFormat;

use super::mutation_lock::MutationLockGuard;

pub struct AppContext {
    pub fmt: OutputFormat,
    pub json_explicit: bool,
    pub quiet: bool,
    pub parallel: bool,
    pub read_only: bool,
    pub config: AppConfig,
}

impl AppContext {
    pub fn new(
        json: bool,
        quiet: bool,
        parallel: bool,
        read_only: bool,
        config_overrides: &[String],
    ) -> Result<Self, CliError> {
        Ok(Self {
            fmt: OutputFormat::detect(json),
            json_explicit: json,
            quiet,
            parallel,
            read_only,
            config: AppConfig::load_with_overrides(config_overrides)?,
        })
    }

    pub async fn client(&self) -> Result<SunoClient, CliError> {
        let auth =
            load_auth_state_with_recovered_environment(self.browser_launch_policy()?).await?;
        SunoClient::new_with_refresh(auth).await
    }

    pub(crate) fn browser_launch_policy(&self) -> Result<BrowserLaunchPolicy, CliError> {
        let bridge_configured = match self.config.challenge_browser {
            ChallengeBrowserMode::Auto => {
                // An unsafe/unknown installation must never grant permission
                // to spawn. Ordinary reads can still use stored request
                // metadata; challenge execution reports the pairing error.
                crate::browser_bridge::bridge_is_configured().unwrap_or(true)
            }
            _ => false,
        };
        Ok(browser_launch_policy(
            self.config.challenge_browser,
            bridge_configured,
        ))
    }

    pub async fn mutation_client(
        &self,
    ) -> Result<(SunoClient, Option<MutationLockGuard>), CliError> {
        self.ensure_mutations_allowed()?;
        let client = self.client().await?;
        let guard = self.acquire_mutation_lock_for(&client.auth_state_snapshot())?;
        client.prepare_mutation_auth().await?;
        Ok((client, guard))
    }

    pub fn ensure_mutations_allowed(&self) -> Result<(), CliError> {
        if self.read_only {
            return Err(CliError::Config(
                "--read-only blocks Suno account mutations; remove it only when this write is intentional"
                    .into(),
            ));
        }
        Ok(())
    }

    pub fn should_lock_mutations(&self) -> bool {
        !self.parallel && self.config.serial_mutations
    }

    pub fn acquire_mutation_lock_for(
        &self,
        auth: &AuthState,
    ) -> Result<Option<MutationLockGuard>, CliError> {
        self.ensure_mutations_allowed()?;
        if !self.should_lock_mutations() {
            return Ok(None);
        }

        MutationLockGuard::acquire(auth).map(Some)
    }
}

fn browser_launch_policy(
    mode: ChallengeBrowserMode,
    bridge_configured: bool,
) -> BrowserLaunchPolicy {
    match mode {
        ChallengeBrowserMode::Existing => BrowserLaunchPolicy::Forbidden,
        ChallengeBrowserMode::Auto if bridge_configured => BrowserLaunchPolicy::Forbidden,
        ChallengeBrowserMode::Auto | ChallengeBrowserMode::Isolated => BrowserLaunchPolicy::Allowed,
    }
}

#[cfg(test)]
mod tests {
    use crate::core::AppConfig;
    use crate::output::OutputFormat;

    use super::{AppContext, BrowserLaunchPolicy, ChallengeBrowserMode, browser_launch_policy};

    #[test]
    fn metadata_probe_obeys_the_same_browser_process_policy_as_challenges() {
        assert_eq!(
            browser_launch_policy(ChallengeBrowserMode::Existing, false),
            BrowserLaunchPolicy::Forbidden
        );
        assert_eq!(
            browser_launch_policy(ChallengeBrowserMode::Auto, true),
            BrowserLaunchPolicy::Forbidden
        );
        assert_eq!(
            browser_launch_policy(ChallengeBrowserMode::Auto, false),
            BrowserLaunchPolicy::Allowed
        );
        assert_eq!(
            browser_launch_policy(ChallengeBrowserMode::Isolated, true),
            BrowserLaunchPolicy::Allowed
        );
    }

    fn context(parallel: bool, serial_mutations: bool, read_only: bool) -> AppContext {
        let config = AppConfig {
            serial_mutations,
            ..Default::default()
        };
        AppContext {
            fmt: OutputFormat::Json,
            json_explicit: true,
            quiet: true,
            parallel,
            read_only,
            config,
        }
    }

    #[test]
    fn mutation_lock_policy_respects_parallel_override() {
        assert!(!context(true, true, false).should_lock_mutations());
    }

    #[test]
    fn mutation_lock_policy_respects_serial_mutations_config() {
        assert!(!context(false, false, false).should_lock_mutations());
    }

    #[test]
    fn mutation_lock_policy_defaults_to_locking() {
        assert!(context(false, true, false).should_lock_mutations());
    }

    #[test]
    fn read_only_rejects_mutations_before_locking() {
        let error = context(false, true, true)
            .ensure_mutations_allowed()
            .expect_err("read-only mode must reject account writes");

        assert!(
            matches!(error, crate::core::CliError::Config(message) if message.contains("--read-only"))
        );
    }
}
