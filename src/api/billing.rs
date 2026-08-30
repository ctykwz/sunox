use super::SunoClient;
use super::types::BillingInfo;
use crate::core::CliError;

impl SunoClient {
    /// Prove that the active JWT is accepted before any non-idempotent write.
    /// This read may refresh a server-stale JWT through `with_auth_retry`, so
    /// the subsequent write can remain strictly single-shot.
    pub(crate) async fn prepare_mutation_auth(&self) -> Result<(), CliError> {
        self.billing_info().await.map(drop)
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
