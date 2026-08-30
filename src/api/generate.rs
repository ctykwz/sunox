use super::SunoClient;
use super::types::{
    Clip, FeedFilters, FeedResponse, FeedV3Request, GenerateRequest, GenerateResponse,
    GenerationResult, MaxLengths, Model,
};
use crate::core::{CliError, MutationAmbiguity};

const CREATE_CONTROL_SLIDERS_FEATURE: &str = "create_control_sliders";
pub(crate) const TAG_UPSAMPLE_FEATURE: &str = "tag_upsample";

impl SunoClient {
    /// Submit a music generation request (custom mode or inspiration mode).
    /// Posts only to the current `/api/generate/v2-web/` route. The older
    /// `/api/generate/v2/` route returned `Token validation failed` after Suno
    /// migrated creates to `v2-web` server-side in the April 2026 capture.
    /// Authentication is refreshed before the mutation client is returned;
    /// the submit itself is never replayed after an auth rejection.
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
        if req.task.as_deref() == Some("gen_stem") {
            return self.prepare_stem_generation_request(req).await;
        }
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
        let mut web_requirement = generation_web_requirement(req.task.as_deref());

        let info = match self.billing_info().await {
            Ok(info) => info,
            Err(error) if is_transient_billing_transport(&error) => {
                if req.duration.is_some() {
                    return Err(CliError::Config(
                        "could not verify --duration against the current Suno billing model and its exact v5.5 limits; refusing to submit without live account model validation"
                            .into(),
                    ));
                }
                if let Some(requirement) = web_requirement {
                    if requirement.task == "cover" {
                        return Err(CliError::Config(
                            "could not verify whether the selected base model is available for the current Suno Cover protocol; refusing to submit a Cover request without validating and mapping its model"
                                .into(),
                        ));
                    }
                    return Err(CliError::Config(format!(
                        "could not verify whether the selected model supports the current Suno Web {} protocol; refusing to submit without account capability validation",
                        requirement.label
                    )));
                }
                if req.metadata.control_sliders.is_some() || !required_features.is_empty() {
                    return Err(CliError::Config(
                        "could not verify whether the selected Suno model supports the requested Create features; refusing to submit a request the Web client would not offer"
                            .into(),
                    ));
                }
                if req.mv == "auto" {
                    return Err(CliError::Config(
                        "could not resolve the account default generation model because Suno billing info is unavailable; refusing to submit without live account model validation"
                            .into(),
                    ));
                }
                return Err(CliError::Config(format!(
                    "could not verify model selector `{}` against the current Suno account; refusing to submit without exact billing validation",
                    req.mv
                )));
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
        let sourced_vox = req.task.as_deref() == Some("vox") && req.artist_clip_id.is_some();
        let model = match select_generation_model(&info.models, requested_model, web_requirement) {
            Ok(model)
                if sourced_vox
                    && web_requirement
                        .is_some_and(|required| !model_supports_requirement(model, required)) =>
            {
                let legacy_requirement = generation_web_requirement(Some("artist_consistency"))
                    .expect("artist_consistency has a Web requirement");
                match select_generation_model(
                    &info.models,
                    requested_model,
                    Some(legacy_requirement),
                ) {
                    Ok(legacy_model)
                        if model_supports_requirement(legacy_model, legacy_requirement) =>
                    {
                        req.task = Some("artist_consistency".into());
                        web_requirement = Some(legacy_requirement);
                        legacy_model
                    }
                    _ => model,
                }
            }
            Ok(model) => model,
            Err(vox_error) if sourced_vox => {
                let legacy_requirement = generation_web_requirement(Some("artist_consistency"))
                    .expect("artist_consistency has a Web requirement");
                match select_generation_model(
                    &info.models,
                    requested_model,
                    Some(legacy_requirement),
                ) {
                    Ok(model) if model_supports_requirement(model, legacy_requirement) => {
                        req.task = Some("artist_consistency".into());
                        web_requirement = Some(legacy_requirement);
                        model
                    }
                    Ok(_) | Err(_) => return Err(vox_error),
                }
            }
            Err(error) => return Err(error),
        };
        if let Some(requirement) = web_requirement
            && !model_supports_requirement(model, requirement)
        {
            return Err(CliError::Config(format!(
                "Suno model `{}` does not support {} in the current Web model capabilities and condition combinations",
                model.external_key, requirement.label
            )));
        }
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
        validate_generation_duration(req, model)?;
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
            .submit_generation_body(&body, req.token.is_some(), &req.transaction_uuid)
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
        self.submit_generation_body(&body, req.token.is_some(), &req.transaction_uuid)
            .await
    }

    async fn submit_generation_body(
        &self,
        body: &serde_json::Value,
        has_challenge_token: bool,
        transaction_uuid: &str,
    ) -> Result<GenerationResult, CliError> {
        let request = self
            .post_without_redirect("/api/generate/v2-web/")
            .json(body);
        let resp = self
            .prepare_mutation_request(request)
            .await?
            .send()
            .await
            .map_err(|error| {
                ambiguous_generation_submit(
                    transaction_uuid,
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        if resp.status().is_redirection() || resp.status().is_server_error() {
            let status = resp.status();
            let response_body = resp.text().await.unwrap_or_default();
            return Err(ambiguous_generation_submit(
                transaction_uuid,
                "response_status",
                "http_error",
                format!("HTTP {status}: {response_body}"),
            ));
        }
        let resp = self
            .check_generation_response(resp, has_challenge_token)
            .await?;
        let raw: serde_json::Value = resp.json().await.map_err(|error| {
            ambiguous_generation_submit(
                transaction_uuid,
                "response_body",
                "http_error",
                error.to_string(),
            )
        })?;
        let result: GenerateResponse = serde_json::from_value(raw.clone()).map_err(|error| {
            ambiguous_generation_submit(
                transaction_uuid,
                "response_schema",
                "json_error",
                error.to_string(),
            )
        })?;
        result.into_result(raw).map_err(|error| {
            ambiguous_generation_submit(
                transaction_uuid,
                "response_schema",
                error.error_code(),
                error.to_string(),
            )
        })
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

    pub(crate) async fn get_clip(&self, id: &str) -> Result<Option<Clip>, CliError> {
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

fn ambiguous_generation_submit(
    transaction_uuid: &str,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "generation transaction {transaction_uuid} lost a reliable response during {stage}; Suno may still have created clips"
        ),
        "generation_submit",
        transaction_uuid,
        stage,
        cause_code,
        cause_message,
        false,
        "a fresh submit would use a new transaction UUID and may duplicate clips or credit usage",
        vec![
            "sunox clip list --json".into(),
            "sunox credits --json".into(),
        ],
    )
    .with_context(
        "transaction_uuid",
        serde_json::Value::String(transaction_uuid.to_string()),
    )
    .into_error()
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

#[derive(Clone, Copy)]
struct WebModelRequirement {
    task: &'static str,
    conditions: &'static [&'static str],
    label: &'static str,
}

fn generation_web_requirement(task: Option<&str>) -> Option<WebModelRequirement> {
    match task {
        Some("cover") => Some(WebModelRequirement {
            task: "cover",
            conditions: &["cover"],
            label: "Cover",
        }),
        Some("extend") => Some(WebModelRequirement {
            task: "extend",
            conditions: &["extend"],
            label: "Extend",
        }),
        Some("upload_extend") => Some(WebModelRequirement {
            task: "upload_extend",
            conditions: &["extend"],
            label: "uploaded-audio Extend",
        }),
        Some("playlist_condition") => Some(WebModelRequirement {
            task: "playlist_condition",
            conditions: &["playlist"],
            label: "Inspiration",
        }),
        Some("vox") => Some(WebModelRequirement {
            task: "vox",
            conditions: &["vox"],
            label: "Voice Persona generation",
        }),
        Some("artist_consistency") => Some(WebModelRequirement {
            task: "artist_consistency",
            conditions: &["persona"],
            label: "legacy Persona generation",
        }),
        _ => None,
    }
}

fn model_supports_requirement(model: &Model, requirement: WebModelRequirement) -> bool {
    model.supports_web_task(requirement.task)
        && model.supports_web_conditions(requirement.conditions)
}

fn select_generation_model<'a>(
    models: &'a [Model],
    requested: &str,
    requirement: Option<WebModelRequirement>,
) -> Result<&'a Model, CliError> {
    let eligible = |model: &&Model| {
        model.can_use
            && requirement
                .map(|required| model_supports_requirement(model, required))
                .unwrap_or(true)
    };
    let selected = if requested == "auto" {
        models
            .iter()
            .filter(eligible)
            .find(|model| model.is_default_model)
            .or_else(|| {
                models
                    .iter()
                    .filter(eligible)
                    .find(|model| model.is_default_free_model)
            })
            .or_else(|| models.iter().find(eligible))
    } else if let Some(model) = models.iter().find(|model| model.external_key == requested) {
        Some(model)
    } else if let Some(model) = models
        .iter()
        .find(|model| generation_model_account_id(model) == Some(requested))
    {
        Some(model)
    } else {
        let display_matches = models
            .iter()
            .filter(|model| model.name.eq_ignore_ascii_case(requested))
            .collect::<Vec<_>>();
        if display_matches.len() > 1 {
            let choices = display_matches
                .iter()
                .map(|model| match generation_model_account_id(model) {
                    Some(id) => format!("{} (id: {id})", model.external_key),
                    None => model.external_key.clone(),
                })
                .collect::<Vec<_>>()
                .join(", ");
            return Err(CliError::Config(format!(
                "generation model display name `{requested}` is ambiguous; select an exact external key or account model ID: {choices}"
            )));
        }
        display_matches.into_iter().next()
    };

    let unavailable = || {
        let requested = if requested == "auto" {
            "an account default model".to_string()
        } else {
            format!("model `{requested}`")
        };
        CliError::Config(format!(
            "Suno account cannot use {requested}; run `sunox models --json` and select a model whose can_use field is true"
        ))
    };
    let Some(model) = selected else {
        return Err(unavailable());
    };
    if !model.can_use {
        return Err(unavailable());
    }
    Ok(model)
}

fn generation_model_account_id(model: &Model) -> Option<&str> {
    ["id", "model_id"]
        .into_iter()
        .find_map(|key| model.extra.get(key).and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn validate_generation_duration(req: &GenerateRequest, model: &Model) -> Result<(), CliError> {
    let Some(duration) = req.duration else {
        return Ok(());
    };
    if !duration.is_finite() || duration <= 0.0 {
        return Err(CliError::Config(
            "generation duration must be a positive finite number of seconds".into(),
        ));
    }
    if model.external_key != "chirp-fenix" {
        return Err(CliError::Config(format!(
            "--duration is only supported by the current v5.5 generation model `chirp-fenix`; selector resolved to `{}`",
            model.external_key
        )));
    }
    let Some(raw_limit) = model.max_lengths.extra.get("duration") else {
        return Ok(());
    };
    if raw_limit.is_null() {
        return Ok(());
    }
    let Some(limit) = raw_limit
        .as_f64()
        .filter(|limit| limit.is_finite() && *limit > 0.0)
    else {
        return Err(CliError::Config(
            "Suno billing returned an invalid max_lengths.duration for the selected model; refusing to guess a duration limit"
                .into(),
        ));
    };
    if duration > limit {
        return Err(CliError::Config(format!(
            "requested duration {duration} seconds exceeds the current account limit of {limit} seconds for `chirp-fenix`"
        )));
    }
    Ok(())
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
        cover_base_model, cover_reference_model, generation_web_requirement,
        select_generation_model, validate_generation_duration, validate_generation_lengths,
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
            allowed_condition_combinations: Vec::new(),
            features: Vec::new(),
            badges: Vec::new(),
            max_lengths,
            extra: Default::default(),
        }
    }

    #[test]
    fn account_model_selection_rejects_unusable_explicit_model() {
        let models = [model(false, true, MaxLengths::default())];

        let error = select_generation_model(&models, "chirp-auk-turbo", None)
            .expect_err("unusable model must be rejected");

        assert!(error.to_string().contains("cannot use"));
    }

    #[test]
    fn account_model_selection_resolves_display_name_and_account_id() {
        let mut custom = model(true, false, MaxLengths::default());
        custom.name = "My Custom Voice".into();
        custom.external_key = "chirp-custom-7".into();
        custom
            .extra
            .insert("id".into(), serde_json::json!("model-account-7"));
        let models = [custom];

        let by_name = select_generation_model(&models, "my custom voice", None)
            .expect("case-insensitive display name");
        let by_id = select_generation_model(&models, "model-account-7", None)
            .expect("exact account model ID");

        assert_eq!(by_name.external_key, "chirp-custom-7");
        assert_eq!(by_id.external_key, "chirp-custom-7");
    }

    #[test]
    fn duplicate_generation_model_display_name_is_ambiguous() {
        let mut first = model(true, false, MaxLengths::default());
        first.name = "My Model".into();
        first.external_key = "chirp-custom-a".into();
        let mut second = model(true, false, MaxLengths::default());
        second.name = "my model".into();
        second.external_key = "chirp-custom-b".into();

        let error = select_generation_model(&[first, second], "MY MODEL", None)
            .expect_err("duplicate display names require an exact selector");

        assert!(error.to_string().contains("is ambiguous"));
        assert!(error.to_string().contains("chirp-custom-a"));
        assert!(error.to_string().contains("chirp-custom-b"));
    }

    #[test]
    fn account_model_selection_uses_the_web_free_default_fallback() {
        let first_usable = model(true, false, MaxLengths::default());
        let mut free_default = model(true, false, MaxLengths::default());
        free_default.external_key = "chirp-fenix".into();
        free_default.is_default_free_model = true;
        let models = [first_usable, free_default];

        let selected = select_generation_model(&models, "auto", None).expect("free default");

        assert_eq!(selected.external_key, "chirp-fenix");
    }

    #[test]
    fn account_model_selection_uses_the_first_usable_model_as_the_final_web_fallback() {
        let mut first_usable = model(true, false, MaxLengths::default());
        first_usable.external_key = "chirp-fenix".into();
        let web_fallback = model(true, false, MaxLengths::default());
        let models = [first_usable, web_fallback];

        let selected = select_generation_model(&models, "auto", None).expect("first usable");

        assert_eq!(selected.external_key, "chirp-fenix");
    }

    #[test]
    fn auto_model_selection_skips_a_default_that_cannot_run_the_requested_web_flow() {
        let mut incompatible_default = model(true, true, MaxLengths::default());
        incompatible_default.capabilities = vec!["generate".into()];
        incompatible_default.allowed_condition_combinations = vec![vec![]];
        let mut compatible_fallback = model(true, false, MaxLengths::default());
        compatible_fallback.external_key = "chirp-fenix".into();
        compatible_fallback.capabilities = vec!["upload_extend".into()];
        compatible_fallback.allowed_condition_combinations = vec![vec!["extend".into()]];
        let models = [incompatible_default, compatible_fallback];

        let selected = select_generation_model(
            &models,
            "auto",
            generation_web_requirement(Some("upload_extend")),
        )
        .expect("compatible upload-extend fallback");

        assert_eq!(selected.external_key, "chirp-fenix");
    }

    #[test]
    fn persona_tasks_require_the_current_web_condition_families() {
        let vox = generation_web_requirement(Some("vox")).expect("Vox requirement");
        assert_eq!(vox.task, "vox");
        assert_eq!(vox.conditions, ["vox"]);

        let legacy = generation_web_requirement(Some("artist_consistency"))
            .expect("legacy Persona requirement");
        assert_eq!(legacy.task, "artist_consistency");
        assert_eq!(legacy.conditions, ["persona"]);
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

    #[test]
    fn duration_requires_exact_v55_and_uses_account_limit_when_present() {
        let mut limits = MaxLengths::default();
        limits
            .extra
            .insert("duration".into(), serde_json::json!(480));
        let mut fenix = model(true, true, limits);
        fenix.name = "v5.5".into();
        fenix.external_key = "chirp-fenix".into();
        let mut request = GenerateRequest::new("chirp-fenix", "custom");
        request.duration = Some(480.0);

        validate_generation_duration(&request, &fenix).expect("duration at account limit");

        request.duration = Some(480.1);
        let error = validate_generation_duration(&request, &fenix)
            .expect_err("duration beyond account limit");
        assert!(
            error
                .to_string()
                .contains("exceeds the current account limit")
        );
    }

    #[test]
    fn duration_does_not_guess_an_upper_bound_when_billing_omits_it() {
        let mut fenix = model(true, true, MaxLengths::default());
        fenix.name = "v5.5".into();
        fenix.external_key = "chirp-fenix".into();
        let mut request = GenerateRequest::new("chirp-fenix", "simple");
        request.duration = Some(900.0);

        validate_generation_duration(&request, &fenix).expect("billing omitted the duration limit");
    }

    #[test]
    fn duration_rejects_non_v55_models() {
        let crow = model(true, true, MaxLengths::default());
        let mut request = GenerateRequest::new("chirp-auk-turbo", "custom");
        request.duration = Some(120.0);

        let error = validate_generation_duration(&request, &crow)
            .expect_err("duration is exclusive to exact current v5.5");

        assert!(error.to_string().contains("only supported"));
    }
}
