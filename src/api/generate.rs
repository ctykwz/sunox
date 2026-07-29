use super::SunoClient;
use super::types::{
    Clip, FeedFilters, FeedResponse, FeedV3Request, GenerateRequest, GenerateResponse,
    GenerationResult, MaxLengths, Model,
};
use crate::core::CliError;

const CREATE_CONTROL_SLIDERS_FEATURE: &str = "create_control_sliders";
pub(crate) const TAG_UPSAMPLE_FEATURE: &str = "tag_upsample";
const WEB_FALLBACK_MODEL: &str = "chirp-auk-turbo";

impl SunoClient {
    /// Submit a music generation request (custom mode or inspiration mode).
    /// Posts only to the current `/api/generate/v2-web/` route. The older
    /// `/api/generate/v2/` route returned `Token validation failed` after Suno
    /// migrated creates to `v2-web` server-side in the April 2026 capture.
    /// Wrapped in `with_auth_retry` so a single stale-JWT failure recovers
    /// transparently via Clerk refresh.
    #[cfg(test)]
    pub async fn generate(&self, req: &GenerateRequest) -> Result<Vec<Clip>, CliError> {
        let mut req = req.clone();
        self.prepare_generation_request(&mut req).await?;
        self.submit_prepared_generation(&req).await
    }

    pub(crate) async fn prepare_generation_request(
        &self,
        req: &mut GenerateRequest,
    ) -> Result<(), CliError> {
        self.prepare_generation_request_internal(req, &[])
            .await
            .map(drop)
    }

    pub(crate) async fn prepare_generation_request_with_features(
        &self,
        req: &mut GenerateRequest,
        required_features: &[&str],
    ) -> Result<MaxLengths, CliError> {
        if required_features.is_empty() {
            return Err(CliError::Config(
                "feature-aware generation preparation requires at least one Web feature".into(),
            ));
        }
        self.prepare_generation_request_internal(req, required_features)
            .await?
            .ok_or_else(|| {
                CliError::Config(
                    "feature-aware generation preparation did not resolve account model limits"
                        .into(),
                )
            })
    }

    async fn prepare_generation_request_internal(
        &self,
        req: &mut GenerateRequest,
        required_features: &[&str],
    ) -> Result<Option<MaxLengths>, CliError> {
        let needs_model_features = req.mv == "auto"
            || req.metadata.control_sliders.is_some()
            || !required_features.is_empty();
        if !req.metadata.user_tier.trim().is_empty() && !needs_model_features {
            return Ok(None);
        }

        let info = match self.billing_info().await {
            Ok(info) => info,
            Err(error) if is_transient_billing_transport(&error) => {
                if req.task.as_deref() == Some("cover") {
                    return Err(CliError::Config(
                        "could not verify whether the selected base model is available for the current Suno Cover protocol; refusing to submit a Cover request without validating and mapping its model"
                            .into(),
                    ));
                }
                if req.metadata.control_sliders.is_some() || !required_features.is_empty() {
                    return Err(CliError::Config(
                        "could not verify whether the selected Suno model supports the requested Create features; refusing to submit a request the Web client would not offer"
                            .into(),
                    ));
                }
                if req.mv == "auto" {
                    req.mv = WEB_FALLBACK_MODEL.into();
                }
                return Ok(None);
            }
            Err(error) => return Err(error),
        };

        if req.metadata.user_tier.trim().is_empty()
            && let Some(user_tier) = info.plan.id
            && !user_tier.trim().is_empty()
        {
            req.metadata.user_tier = user_tier.trim().to_string();
        }

        if info.models.is_empty() {
            return Err(CliError::Config(
                "Suno billing info returned no generation models; refusing to guess after a successful account capability lookup".into(),
            ));
        }

        let requested_model = if req.task.as_deref() == Some("cover") {
            cover_base_model(&req.mv)
        } else {
            &req.mv
        };
        let model = select_generation_model(&info.models, requested_model)?;
        req.mv = if req.task.as_deref() == Some("cover") {
            cover_reference_model(&model.external_key).to_string()
        } else {
            model.external_key.clone()
        };
        if req.metadata.control_sliders.is_some()
            && !model.supports_web_feature(CREATE_CONTROL_SLIDERS_FEATURE)
        {
            return Err(CliError::Config(format!(
                "Suno model `{}` does not support --weirdness/--style-influence; refusing to submit while preserving the requested controls",
                model.external_key
            )));
        }
        for feature in required_features {
            if !model.supports_web_feature(feature) {
                return Err(CliError::Config(format!(
                    "Suno model `{}` does not support Web Create feature `{feature}`",
                    model.external_key
                )));
            }
        }
        let uses_account_generation_limits =
            matches!(req.task.as_deref(), None | Some("playlist_condition"));
        if uses_account_generation_limits {
            validate_generation_lengths(req, model)?;
        }
        Ok(Some(model.max_lengths.clone()))
    }

    #[cfg(test)]
    pub(crate) async fn submit_prepared_generation(
        &self,
        req: &GenerateRequest,
    ) -> Result<Vec<Clip>, CliError> {
        self.ensure_generation_challenge(req.token.is_some())
            .await?;
        let body = serde_json::to_value(req)?;
        Ok(self
            .submit_generation_body(&body, req.token.is_some())
            .await?
            .clips)
    }

    #[cfg(test)]
    async fn ensure_generation_challenge(&self, has_token: bool) -> Result<(), CliError> {
        if !has_token {
            let challenge = self.generation_challenge_with_refresh().await?;
            if challenge.required {
                return Err(generation_challenge_error(&challenge));
            }
        }
        Ok(())
    }

    pub(crate) async fn submit_prepared_generation_after_challenge(
        &self,
        req: &GenerateRequest,
    ) -> Result<GenerationResult, CliError> {
        let body = serde_json::to_value(req)?;
        self.submit_generation_body(&body, req.token.is_some())
            .await
    }

    async fn submit_generation_body(
        &self,
        body: &serde_json::Value,
        has_challenge_token: bool,
    ) -> Result<GenerationResult, CliError> {
        self.with_auth_retry(|| async {
            let resp = self.post("/api/generate/v2-web/").json(body).send().await?;
            let resp = self
                .check_generation_response(resp, has_challenge_token)
                .await?;
            let raw: serde_json::Value = resp.json().await?;
            let result: GenerateResponse = serde_json::from_value(raw.clone())?;
            result.into_result(raw)
        })
        .await
    }

    /// Fetch clips by IDs using the same split as the current Web client:
    /// direct `/api/clip/{id}` for a single detail read and batched feed/v3
    /// exact-ID filters for generation polling and other multi-clip reads.
    /// Temporarily missing clips are omitted so polling callers can retry them.
    pub async fn get_clips(&self, ids: &[String]) -> Result<Vec<Clip>, CliError> {
        const WEB_POLL_BATCH_SIZE: usize = 48;

        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if ids.len() == 1 {
            return Ok(self.get_clip(&ids[0]).await?.into_iter().collect());
        }

        let mut by_id = std::collections::HashMap::with_capacity(ids.len());
        for batch in ids.chunks(WEB_POLL_BATCH_SIZE) {
            let req = FeedV3Request {
                cursor: None,
                limit: Some(batch.len() as u32),
                filters: Some(FeedFilters::ids(batch)),
            };
            let response: FeedResponse = self
                .with_auth_retry(|| async {
                    let resp = self.post("/api/feed/v3").json(&req).send().await?;
                    let resp = self.check_response(resp).await?;
                    Ok(resp.json().await?)
                })
                .await?;
            by_id.extend(
                response
                    .clips
                    .into_iter()
                    .map(|clip| (clip.id.clone(), clip)),
            );
        }

        Ok(ids.iter().filter_map(|id| by_id.get(id).cloned()).collect())
    }

    async fn get_clip(&self, id: &str) -> Result<Option<Clip>, CliError> {
        let path = format!("/api/clip/{id}");
        self.with_auth_retry(|| async {
            let resp = self.get(&path).send().await?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            let resp = self.check_response(resp).await?;
            let value: serde_json::Value = resp.json().await?;
            if value.is_null() {
                return Ok(None);
            }
            Ok(Some(serde_json::from_value::<Clip>(value)?))
        })
        .await
    }
}

pub(crate) fn is_transient_billing_transport(error: &CliError) -> bool {
    matches!(
        error,
        CliError::Http(error) if error.is_connect() || error.is_timeout()
    )
}

fn cover_base_model(model: &str) -> &str {
    match model {
        "chirp-v3-5-tau" => "chirp-v3-5",
        "chirp-v4-tau" => "chirp-v4",
        model => model,
    }
}

fn cover_reference_model(model: &str) -> &str {
    match model {
        "chirp-v3-0" | "chirp-v3-5" => "chirp-v3-5-tau",
        "chirp-v4" => "chirp-v4-tau",
        model => model,
    }
}

fn select_generation_model<'a>(
    models: &'a [Model],
    requested: &str,
) -> Result<&'a Model, CliError> {
    let selected = if requested == "auto" {
        models
            .iter()
            .find(|model| model.can_use && model.is_default_model)
            .or_else(|| {
                models
                    .iter()
                    .find(|model| model.can_use && model.is_default_free_model)
            })
            .or_else(|| models.iter().find(|model| model.can_use))
    } else {
        models
            .iter()
            .find(|model| model.external_key == requested && model.can_use)
    };

    selected.ok_or_else(|| {
        let requested = if requested == "auto" {
            "an account default model".to_string()
        } else {
            format!("model `{requested}`")
        };
        CliError::Config(format!(
            "Suno account cannot use {requested}; run `sunox models --json` and select a model whose can_use field is true"
        ))
    })
}

fn validate_generation_lengths(req: &GenerateRequest, model: &Model) -> Result<(), CliError> {
    validate_generation_lengths_with_limits(req, &model.max_lengths)
}

pub(crate) fn validate_generation_lengths_with_limits(
    req: &GenerateRequest,
    limits: &MaxLengths,
) -> Result<(), CliError> {
    validate_length("title", req.title.as_deref(), limits.title)?;
    validate_length("prompt", Some(&req.prompt), limits.prompt)?;
    validate_length("tags", req.tags.as_deref(), limits.tags)?;
    validate_length(
        "negative_tags",
        Some(&req.negative_tags),
        limits.negative_tags,
    )?;
    validate_length(
        "gpt_description_prompt",
        req.gpt_description_prompt.as_deref(),
        limits.gpt_description_prompt,
    )
}

fn validate_length(field: &str, value: Option<&str>, limit: u32) -> Result<(), CliError> {
    let length = value.map(|value| value.chars().count()).unwrap_or(0);
    if limit > 0 && length > limit as usize {
        return Err(CliError::Config(format!(
            "generation field `{field}` is {length} characters, exceeding the account limit of {limit} for the selected model"
        )));
    }
    Ok(())
}

#[cfg(test)]
fn generation_challenge_error(challenge: &super::challenge::GenerationChallenge) -> CliError {
    let version = challenge
        .captcha_version
        .map(|version| version.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    CliError::ChallengeRequired(format!(
        "Suno requires a generation challenge (captcha_version={version}). When stored Clerk refresh material is available, Sunox refreshes the JWT once and repeats the challenge preflight before showing this message. Complete a manual generation challenge in the Suno web app and retry, provide a valid challenge token with --token <token>, or force the browser-backed solver with --captcha."
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        cover_base_model, cover_reference_model, select_generation_model,
        validate_generation_lengths,
    };
    use crate::api::types::{GenerateRequest, MaxLengths, Model};

    fn model(can_use: bool, is_default_model: bool, max_lengths: MaxLengths) -> Model {
        Model {
            name: "v4.5-all".into(),
            external_key: "chirp-auk-turbo".into(),
            can_use,
            is_default_model,
            is_default_free_model: false,
            description: "fixture".into(),
            capabilities: Vec::new(),
            features: Vec::new(),
            badges: Vec::new(),
            max_lengths,
            extra: Default::default(),
        }
    }

    #[test]
    fn account_model_selection_rejects_unusable_explicit_model() {
        let models = [model(false, true, MaxLengths::default())];

        let error = select_generation_model(&models, "chirp-auk-turbo")
            .expect_err("unusable model must be rejected");

        assert!(error.to_string().contains("cannot use"));
    }

    #[test]
    fn account_model_selection_uses_the_web_free_default_fallback() {
        let first_usable = model(true, false, MaxLengths::default());
        let mut free_default = model(true, false, MaxLengths::default());
        free_default.external_key = "chirp-fenix".into();
        free_default.is_default_free_model = true;
        let models = [first_usable, free_default];

        let selected = select_generation_model(&models, "auto").expect("free default");

        assert_eq!(selected.external_key, "chirp-fenix");
    }

    #[test]
    fn account_model_selection_uses_the_first_usable_model_as_the_final_web_fallback() {
        let mut first_usable = model(true, false, MaxLengths::default());
        first_usable.external_key = "chirp-fenix".into();
        let web_fallback = model(true, false, MaxLengths::default());
        let models = [first_usable, web_fallback];

        let selected = select_generation_model(&models, "auto").expect("first usable");

        assert_eq!(selected.external_key, "chirp-fenix");
    }

    #[test]
    fn cover_reference_models_match_the_current_web_mapping() {
        assert_eq!(cover_base_model("chirp-v3-5-tau"), "chirp-v3-5");
        assert_eq!(cover_base_model("chirp-v4-tau"), "chirp-v4");
        assert_eq!(cover_reference_model("chirp-v3-0"), "chirp-v3-5-tau");
        assert_eq!(cover_reference_model("chirp-v3-5"), "chirp-v3-5-tau");
        assert_eq!(cover_reference_model("chirp-v4"), "chirp-v4-tau");
        assert_eq!(cover_reference_model("chirp-auk-turbo"), "chirp-auk-turbo");
        assert_eq!(cover_reference_model("chirp-fenix"), "chirp-fenix");
    }

    #[test]
    fn generation_limits_count_characters_instead_of_utf8_bytes() {
        let selected = model(
            true,
            true,
            MaxLengths {
                title: 2,
                ..MaxLengths::default()
            },
        );
        let mut request = GenerateRequest::new("chirp-auk-turbo", "custom");
        request.title = Some("中文歌".into());

        let error = validate_generation_lengths(&request, &selected)
            .expect_err("three characters exceed a two-character limit");

        assert!(error.to_string().contains("3 characters"));
    }
}
