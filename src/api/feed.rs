use super::SunoClient;
use super::types::{FeedFilters, FeedResponse, FeedV3Request};
use crate::core::CliError;

impl SunoClient {
    /// List songs using feed/v3 with optional search and filters.
    pub async fn feed(
        &self,
        cursor: Option<String>,
        limit: Option<u32>,
        filters: FeedFilters,
    ) -> Result<FeedResponse, CliError> {
        let req = FeedV3Request {
            cursor,
            limit: Some(limit.unwrap_or(20)),
            filters: Some(filters),
        };
        self.with_auth_retry(|| async {
            let resp = self.post("/api/feed/v3").json(&req).send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Search songs using feed/v3 native searchText filter.
    pub async fn search(&self, query: &str) -> Result<FeedResponse, CliError> {
        self.search_page(query, None, Some(50)).await
    }

    pub async fn search_page(
        &self,
        query: &str,
        cursor: Option<String>,
        limit: Option<u32>,
    ) -> Result<FeedResponse, CliError> {
        let req = FeedV3Request {
            cursor,
            limit: Some(limit.unwrap_or(50)),
            filters: Some(FeedFilters::search(query)),
        };
        self.with_auth_retry(|| async {
            let resp = self.post("/api/feed/v3").json(&req).send().await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}
