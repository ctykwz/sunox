use super::SunoClient;
use super::types::BillingInfo;
use crate::core::CliError;

impl SunoClient {
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
