use super::SunoClient;
use super::types::{Clip, GenerateRequest};
use crate::core::CliError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StemBank {
    pub page: u32,
    pub stems: Vec<super::types::Clip>,
    /// References returned by the stems endpoint that could not be hydrated
    /// through the normal clip-detail/feed reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_clip_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StemResults {
    pub clip_id: String,
    pub pages: u32,
    pub banks: Vec<StemBank>,
}

#[derive(Debug, Deserialize)]
struct StemPagesResponse {
    #[serde(default)]
    pages: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct StemPageResponse {
    #[serde(default)]
    stems: Option<Vec<StemReference>>,
}

#[derive(Debug, Deserialize)]
struct StemReference {
    id: String,
}

#[derive(Debug)]
struct RawStemBank {
    page: u32,
    clip_ids: Vec<String>,
}

impl SunoClient {
    /// Prepare the fixed hidden transport model used by current Web Get Stems.
    /// `chirp-v3-0` is not a user-selectable generation model here, so its
    /// absence from billing.models must not block an otherwise entitled Pro
    /// account.
    pub(crate) async fn prepare_stem_generation_request(
        &self,
        req: &mut GenerateRequest,
    ) -> Result<(), CliError> {
        if req.task.as_deref() != Some("gen_stem")
            || req.mv != "chirp-v3-0"
            || req.stem_type_id != Some(91)
        {
            return Err(CliError::Config(
                "stem generation preparation requires the verified current gen_stem transport contract"
                    .into(),
            ));
        }
        let billing = self.billing_info().await?;
        let entitled = billing
            .accessible_features
            .as_ref()
            .is_some_and(|features| features.contains("get_stems"))
            || billing
                .plan
                .usage_plan_features
                .iter()
                .any(|feature| feature.name == "get_stems");
        if !billing.is_active || !entitled {
            return Err(CliError::Config(
                "the current active Suno plan does not expose the `get_stems` feature".into(),
            ));
        }
        if req.metadata.user_tier.trim().is_empty()
            && let Some(tier) = billing.plan.id
            && !tier.trim().is_empty()
        {
            req.metadata.user_tier = tier.trim().to_string();
        }
        Ok(())
    }

    /// Extract stems from a clip via the current web `gen_stem` generation task.
    #[cfg(test)]
    pub async fn stems(
        &self,
        clip_id: &str,
        challenge_token: Option<String>,
    ) -> Result<Vec<Clip>, CliError> {
        let req = self.prepare_stems_request(clip_id, challenge_token).await?;
        self.generate(&req).await
    }

    pub(crate) async fn prepare_stems_request(
        &self,
        clip_id: &str,
        challenge_token: Option<String>,
    ) -> Result<GenerateRequest, CliError> {
        let requested = [clip_id.to_string()];
        let source = self
            .get_clips(&requested)
            .await?
            .into_iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| CliError::NotFound(format!("clip: {clip_id}")))?;
        validate_stem_source(&source)?;

        let mut req = GenerateRequest::new("chirp-v3-0", "custom");
        req.task = Some("gen_stem".into());
        req.title = Some(source.title);
        req.tags = Some(String::new());
        req.make_instrumental = true;
        req.continue_clip_id = Some(clip_id.to_string());
        req.stem_type_id = Some(91);
        req.stem_type_group_name = Some("Twelve".into());
        req.stem_task = Some("twelve".into());
        req.metadata.is_remix = Some(true);
        req.set_challenge_token(challenge_token);

        Ok(req)
    }

    /// Build the Pro "Split from Mix" request. Unlike Auto Split, this asks
    /// Suno for one named target plus its complement.
    pub(crate) async fn prepare_split_stems_request(
        &self,
        clip_id: &str,
        stem_group: &str,
        stem_name: &str,
        challenge_token: Option<String>,
    ) -> Result<GenerateRequest, CliError> {
        let stem_group = stem_group.trim();
        let stem_name = stem_name.trim();
        if stem_group.is_empty() || stem_name.is_empty() {
            return Err(CliError::Config(
                "Split from Mix requires a current stem group and canonical stem name".into(),
            ));
        }
        let mut req = self.prepare_stems_request(clip_id, challenge_token).await?;
        req.stem_type_group_name = Some(stem_group.to_string());
        req.stem_task = Some("extract".into());
        req.stem_name = Some(stem_name.to_string());
        Ok(req)
    }

    /// Read the number of stored stem-result pages for a source clip.
    pub async fn stem_result_pages(&self, clip_id: &str) -> Result<u32, CliError> {
        let path = format!("/api/clip/{clip_id}/stems/pages");
        self.with_auth_retry(|| async {
            let response: StemPagesResponse =
                self.read_json_with_transport_retry(self.get(&path)).await?;
            Ok(response.pages.unwrap_or_default())
        })
        .await
    }

    /// Read one zero-based page of stored stem results.
    pub async fn get_stem_result_page(
        &self,
        clip_id: &str,
        page: u32,
    ) -> Result<StemBank, CliError> {
        let raw = self.get_stem_reference_page(clip_id, page).await?;
        let clips = self.get_clips(&raw.clip_ids).await?;
        Ok(hydrate_stem_bank(raw, clips))
    }

    async fn get_stem_reference_page(
        &self,
        clip_id: &str,
        page: u32,
    ) -> Result<RawStemBank, CliError> {
        let path = format!("/api/clip/{clip_id}/stems");
        let response: StemPageResponse = self
            .with_auth_retry(|| async {
                self.read_json_with_transport_retry(self.get(&path).query(&[("page", page)]))
                    .await
            })
            .await?;
        let mut clip_ids = Vec::new();
        for reference in response.stems.unwrap_or_default() {
            let id = reference.id.trim();
            if id.is_empty() {
                return Err(CliError::Api {
                    code: "schema_drift",
                    message: format!(
                        "stem-result page {page} for clip {clip_id} contained a blank stem ID"
                    ),
                });
            }
            if !clip_ids.iter().any(|known| known == id) {
                clip_ids.push(id.to_string());
            }
        }
        Ok(RawStemBank { page, clip_ids })
    }

    /// Read every stored stem-result bank without starting a new extraction.
    pub async fn get_all_stem_results(&self, clip_id: &str) -> Result<StemResults, CliError> {
        let pages = self.stem_result_pages(clip_id).await?;
        let mut raw_banks = Vec::with_capacity(pages as usize);
        for page in 0..pages {
            raw_banks.push(self.get_stem_reference_page(clip_id, page).await?);
        }
        let mut all_ids = Vec::new();
        for id in raw_banks.iter().flat_map(|bank| bank.clip_ids.iter()) {
            if !all_ids.iter().any(|known| known == id) {
                all_ids.push(id.clone());
            }
        }
        let clips = self.get_clips(&all_ids).await?;
        let by_id = clips
            .into_iter()
            .map(|clip| (clip.id.clone(), clip))
            .collect::<std::collections::HashMap<_, _>>();
        let banks = raw_banks
            .into_iter()
            .map(|raw| hydrate_stem_bank_from_map(raw, &by_id))
            .collect();
        Ok(StemResults {
            clip_id: clip_id.to_string(),
            pages,
            banks,
        })
    }
}

fn hydrate_stem_bank(raw: RawStemBank, clips: Vec<Clip>) -> StemBank {
    let by_id = clips
        .into_iter()
        .map(|clip| (clip.id.clone(), clip))
        .collect::<std::collections::HashMap<_, _>>();
    hydrate_stem_bank_from_map(raw, &by_id)
}

fn hydrate_stem_bank_from_map(
    raw: RawStemBank,
    by_id: &std::collections::HashMap<String, Clip>,
) -> StemBank {
    let mut stems = Vec::new();
    let mut missing_clip_ids = Vec::new();
    for id in raw.clip_ids {
        match by_id.get(&id) {
            Some(clip) => stems.push(clip.clone()),
            None => missing_clip_ids.push(id),
        }
    }
    StemBank {
        page: raw.page,
        stems,
        missing_clip_ids,
    }
}

fn validate_stem_source(source: &Clip) -> Result<(), CliError> {
    if source.status != "complete" {
        return Err(CliError::Config(format!(
            "source clip `{}` must be complete before stem extraction",
            source.id
        )));
    }
    match source.is_trashed {
        Some(false) => {}
        Some(true) => {
            return Err(CliError::Config(format!(
                "source clip `{}` is trashed and cannot be split into stems",
                source.id
            )));
        }
        None => {
            return Err(CliError::Config(format!(
                "source clip `{}` did not explicitly report is_trashed=false; refusing stem extraction",
                source.id
            )));
        }
    }
    if let Some(reason) = source
        .extra
        .get("download_disabled_reason")
        .filter(|reason| match reason {
            serde_json::Value::Null => false,
            serde_json::Value::String(reason) => !reason.trim().is_empty(),
            _ => true,
        })
    {
        return Err(CliError::Config(format!(
            "source clip `{}` is not eligible for Get Stems: download_disabled_reason={reason}",
            source.id
        )));
    }
    let get_stems = source
        .action_config
        .as_ref()
        .and_then(|config| config.action("get_stems"));
    if !get_stems
        .is_some_and(|action| action.visible == Some(true) && action.disabled == Some(false))
    {
        return Err(CliError::Config(format!(
            "source clip `{}` does not expose an enabled Get Stems action for this account",
            source.id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_stem_source;
    use crate::api::types::Clip;

    fn source(action_type: &str, visible: bool, disabled: bool) -> Clip {
        serde_json::from_value(serde_json::json!({
            "id": "clip-a",
            "title": "Source",
            "status": "complete",
            "model_name": "chirp-fenix",
            "created_at": "2026-08-24T00:00:00Z",
            "is_trashed": false,
            "action_config": {"actions": [{
                "action_type": action_type,
                "visible": visible,
                "disabled": disabled
            }]}
        }))
        .expect("source fixture")
    }

    #[test]
    fn stem_extraction_requires_the_current_enabled_source_action() {
        validate_stem_source(&source("get_stems", true, false)).expect("eligible source");
        for clip in [
            source("get_stems", false, false),
            source("get_stems", true, true),
            source("remaster", true, false),
        ] {
            let error = validate_stem_source(&clip).expect_err("source action must gate stems");
            assert!(error.to_string().contains("Get Stems action"));
        }
    }

    #[test]
    fn stem_extraction_requires_complete_non_trashed_source_state() {
        let mut incomplete = source("get_stems", true, false);
        incomplete.status = "processing".into();
        assert!(validate_stem_source(&incomplete).is_err());

        let mut trashed = source("get_stems", true, false);
        trashed.is_trashed = Some(true);
        assert!(validate_stem_source(&trashed).is_err());

        let mut unknown = source("get_stems", true, false);
        unknown.is_trashed = None;
        assert!(validate_stem_source(&unknown).is_err());
    }

    #[test]
    fn stem_extraction_rejects_a_download_disabled_source() {
        let mut disabled = source("get_stems", true, false);
        disabled.extra.insert(
            "download_disabled_reason".into(),
            serde_json::Value::String("rights_restricted".into()),
        );

        let error = validate_stem_source(&disabled).expect_err("rights gate must fail closed");
        assert!(error.to_string().contains("download_disabled_reason"));
    }
}
