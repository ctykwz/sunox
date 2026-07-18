use super::SunoClient;
use super::types::BillingInfo;
use crate::core::CliError;

impl SunoClient {
    pub async fn billing_info(&self) -> Result<BillingInfo, CliError> {
        self.with_auth_retry(|| async {
            let resp = self.get("/api/billing/info/").send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    pub(crate) async fn validate_auth(&self) -> Result<(), CliError> {
        let resp = self.get("/api/billing/info/").send().await?;
        let resp = self.check_response(resp).await?;
        let _: BillingInfo = resp.json().await?;
        Ok(())
    }
}
