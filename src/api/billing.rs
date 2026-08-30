use super::SunoClient;
use super::types::BillingInfo;
use crate::core::CliError;
use std::time::{Duration, Instant};

const MUTATION_AUTH_MAX_AGE: Duration = Duration::from_secs(30);

impl SunoClient {
    /// Prove that the active JWT is accepted before any non-idempotent write.
    /// This read may refresh a server-stale JWT through `with_auth_retry`, so
    /// the subsequent write can remain strictly single-shot.
    pub(crate) async fn prepare_mutation_auth(&self) -> Result<(), CliError> {
        self.billing_info().await?;
        *self
            .mutation_auth_preflight_at
            .lock()
            .expect("mutation auth preflight mutex poisoned") = Some(Instant::now());
        Ok(())
    }

    /// Revalidate authentication before a later write after a potentially
    /// long upload or poll gap. `send_mutation_once` applies this centrally;
    /// direct API/test clients that did not enter through `mutation_client`
    /// remain unchanged.
    pub(crate) async fn refresh_mutation_auth_if_stale(&self) -> Result<(), CliError> {
        let should_refresh = self
            .mutation_auth_preflight_at
            .lock()
            .expect("mutation auth preflight mutex poisoned")
            .is_some_and(|validated_at| validated_at.elapsed() >= MUTATION_AUTH_MAX_AGE);
        if should_refresh {
            self.prepare_mutation_auth().await?;
        }
        Ok(())
    }

    pub async fn billing_info(&self) -> Result<BillingInfo, CliError> {
        self.with_auth_retry(|| async {
            self.read_json_with_transport_retry(self.get("/api/billing/info/"))
                .await
        })
        .await
    }

    pub(crate) async fn validate_auth(&self) -> Result<(), CliError> {
        let _: BillingInfo = self
            .read_json_with_transport_retry(self.get("/api/billing/info/"))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::MUTATION_AUTH_MAX_AGE;

    #[test]
    fn mutation_auth_freshness_window_is_shorter_than_long_workflow_budgets() {
        assert_eq!(MUTATION_AUTH_MAX_AGE.as_secs(), 30);
    }
}
