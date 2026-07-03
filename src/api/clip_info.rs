use super::SunoClient;
use super::types::{
    Clip, ClipAttribution, ClipComments, ClipInfo, ClipInfoSupplementalError,
    DirectChildrenCountResponse, SimilarClipsResponse,
};
use crate::core::CliError;

impl SunoClient {
    /// Fetch the attribution block shown on the Suno song page.
    /// GET /api/clips/{clip_id}/attribution
    pub async fn clip_attribution(&self, clip_id: &str) -> Result<ClipAttribution, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/clips/{clip_id}/attribution"))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Fetch the first page of comments shown on the Suno song page.
    /// GET /api/gen/{clip_id}/comments?order=most_liked
    pub async fn clip_comments(&self, clip_id: &str) -> Result<ClipComments, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get(&format!("/api/gen/{clip_id}/comments"))
                .query(&[("order", "most_liked")])
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }

    /// Fetch the number of direct children derived from a clip.
    /// GET /api/clips/direct_children_count?clip_id={clip_id}
    pub async fn direct_children_count(&self, clip_id: &str) -> Result<u64, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get("/api/clips/direct_children_count")
                .query(&[("clip_id", clip_id)])
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let body: DirectChildrenCountResponse = resp.json().await?;
            Ok(body.count)
        })
        .await
    }

    /// Fetch similar clips shown on the Suno song page.
    /// GET /api/clips/get_similar/?id={clip_id}
    pub async fn similar_clips(&self, clip_id: &str) -> Result<Vec<Clip>, CliError> {
        self.with_auth_retry(|| async {
            let resp = self
                .get("/api/clips/get_similar/")
                .query(&[("id", clip_id)])
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            let body: SimilarClipsResponse = resp.json().await?;
            Ok(body.similar_clips)
        })
        .await
    }

    /// Compose the main feed clip with song-page enrichment reads.
    pub async fn clip_info(&self, clip: Clip) -> Result<ClipInfo, CliError> {
        let clip_id = clip.id.as_str();
        let mut supplemental_errors = Vec::new();
        let attribution = match self.clip_attribution(clip_id).await {
            Ok(value) => value,
            Err(error) => {
                if should_abort_supplemental_error(&error) {
                    return Err(error);
                }
                supplemental_errors.push(supplemental_error("attribution", error));
                ClipAttribution::default()
            }
        };
        let comments = match self.clip_comments(clip_id).await {
            Ok(value) => value,
            Err(error) => {
                if should_abort_supplemental_error(&error) {
                    return Err(error);
                }
                supplemental_errors.push(supplemental_error("comments", error));
                ClipComments::default()
            }
        };
        let direct_children_count = match self.direct_children_count(clip_id).await {
            Ok(value) => value,
            Err(error) => {
                if should_abort_supplemental_error(&error) {
                    return Err(error);
                }
                supplemental_errors.push(supplemental_error("direct_children_count", error));
                0
            }
        };
        let similar_clips = match self.similar_clips(clip_id).await {
            Ok(value) => value,
            Err(error) => {
                if should_abort_supplemental_error(&error) {
                    return Err(error);
                }
                supplemental_errors.push(supplemental_error("similar_clips", error));
                Vec::new()
            }
        };
        Ok(ClipInfo {
            clip,
            attribution,
            comments,
            direct_children_count,
            similar_clips,
            supplemental_errors,
        })
    }
}

fn should_abort_supplemental_error(error: &CliError) -> bool {
    matches!(
        error,
        CliError::AuthMissing | CliError::AuthExpired | CliError::RateLimited
    )
}

fn supplemental_error(field: &str, error: CliError) -> ClipInfoSupplementalError {
    ClipInfoSupplementalError {
        field: field.to_string(),
        code: error.error_code().to_string(),
        message: error.to_string(),
    }
}
