use super::SunoClient;
use super::types::{
    Clip, GenerateResponse, GenerationResult, RemasterStyleProfile, RemasterVariation,
};
use crate::core::{CliError, MutationAmbiguity};
use serde::Serialize;

#[derive(Debug, Default)]
pub struct RemasterOptions {
    pub variation: Option<RemasterVariation>,
    pub style_profile: Option<RemasterStyleProfile>,
}

#[derive(Serialize)]
struct RemasterRequest<'a> {
    clip_id: &'a str,
    model_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    variation_category: Option<RemasterVariation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style_profile: Option<RemasterStyleProfile>,
}

impl SunoClient {
    /// Remaster a clip with a different model version.
    /// Posts to the current web remaster route captured as `/api/generate/upsample`.
    #[cfg(test)]
    pub async fn remaster(
        &self,
        clip_id: &str,
        remaster_model_key: &str,
        requested_variation: Option<RemasterVariation>,
    ) -> Result<GenerationResult, CliError> {
        self.remaster_with_options(
            clip_id,
            remaster_model_key,
            RemasterOptions {
                variation: requested_variation,
                ..RemasterOptions::default()
            },
        )
        .await
    }

    pub async fn remaster_with_options(
        &self,
        clip_id: &str,
        remaster_model_key: &str,
        options: RemasterOptions,
    ) -> Result<GenerationResult, CliError> {
        let variation_category = resolve_variation_category(remaster_model_key, options.variation)?;
        let requested = [clip_id.to_string()];
        let source = self
            .get_clips(&requested)
            .await?
            .into_iter()
            .find(|clip| clip.id == clip_id)
            .ok_or_else(|| CliError::NotFound(format!("exact source clip: {clip_id}")))?;
        validate_remaster_source(&source)?;

        let req = RemasterRequest {
            clip_id,
            model_name: remaster_model_key,
            variation_category,
            style_profile: resolve_style_profile(remaster_model_key, options.style_profile)?,
        };
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = self
            .post_without_redirect("/api/generate/upsample")
            .json(&req);
        let resp = self
            .prepare_mutation_request(request)
            .await?
            .send()
            .await
            .map_err(|error| {
                ambiguous_remaster(
                    &operation_id,
                    clip_id,
                    remaster_model_key,
                    variation_category,
                    "request_send",
                    "http_error",
                    error.to_string(),
                )
            })?;
        if resp.status().is_redirection() || resp.status().is_server_error() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ambiguous_remaster(
                &operation_id,
                clip_id,
                remaster_model_key,
                variation_category,
                "response_status",
                "http_error",
                format!("HTTP {status}: {body}"),
            ));
        }
        let resp = self.check_response(resp).await?;
        let raw: serde_json::Value = resp.json().await.map_err(|error| {
            ambiguous_remaster(
                &operation_id,
                clip_id,
                remaster_model_key,
                variation_category,
                "response_body",
                "http_error",
                error.to_string(),
            )
        })?;
        crate::core::operation::record_response("/api/generate/upsample", &raw).map_err(
            |error| {
                ambiguous_remaster(
                    &operation_id,
                    clip_id,
                    remaster_model_key,
                    variation_category,
                    "checkpoint_persist",
                    error.error_code(),
                    error.to_string(),
                )
            },
        )?;
        let result: GenerateResponse = serde_json::from_value(raw.clone()).map_err(|error| {
            ambiguous_remaster(
                &operation_id,
                clip_id,
                remaster_model_key,
                variation_category,
                "response_schema",
                "json_error",
                error.to_string(),
            )
        })?;
        result.into_result(raw).map_err(|error| {
            ambiguous_remaster(
                &operation_id,
                clip_id,
                remaster_model_key,
                variation_category,
                "response_schema",
                error.error_code(),
                error.to_string(),
            )
        })
    }
}

fn resolve_variation_category(
    remaster_model_key: &str,
    requested: Option<RemasterVariation>,
) -> Result<Option<RemasterVariation>, CliError> {
    match remaster_model_key {
        "chirp-bass" => {
            if requested.is_some() {
                return Err(CliError::Config(
                    "--variation is not supported by the v4.5+ Remaster model".into(),
                ));
            }
            Ok(None)
        }
        "chirp-halibut" => Ok(Some(requested.unwrap_or_default())),
        "chirp-flounder" | "chirp-carp" => Ok(Some(requested.unwrap_or_default())),
        _ => Err(CliError::Config(format!(
            "unsupported Remaster model `{remaster_model_key}`; refusing to guess its variation_category contract"
        ))),
    }
}

fn resolve_style_profile(
    remaster_model_key: &str,
    requested: Option<RemasterStyleProfile>,
) -> Result<Option<RemasterStyleProfile>, CliError> {
    if remaster_model_key == "chirp-halibut" {
        return Ok(Some(requested.unwrap_or_default()));
    }
    if requested.is_some() {
        return Err(CliError::Config(
            "--style-profile is supported only by the v6 Remaster model `chirp-halibut`".into(),
        ));
    }
    Ok(None)
}

fn validate_remaster_source(source: &Clip) -> Result<(), CliError> {
    if source.status != "complete" {
        return Err(CliError::Config(format!(
            "source clip `{}` must be complete before Remaster",
            source.id
        )));
    }
    match source.is_trashed {
        Some(false) => {}
        Some(true) => {
            return Err(CliError::Config(format!(
                "source clip `{}` is trashed and cannot be remastered",
                source.id
            )));
        }
        None => {
            return Err(CliError::Config(format!(
                "source clip `{}` did not explicitly report is_trashed=false; refusing Remaster",
                source.id
            )));
        }
    }
    if source.metadata.infill == Some(true) {
        return Err(CliError::Config(format!(
            "source clip `{}` is an infill and cannot be remastered",
            source.id
        )));
    }
    match source.metadata.duration {
        Some(duration) if duration.is_finite() && (0.0..=960.0).contains(&duration) => {}
        Some(duration) => {
            return Err(CliError::Config(format!(
                "source clip `{}` duration {duration}s is outside the supported 0..=960s Remaster range",
                source.id
            )));
        }
        None => {
            return Err(CliError::Config(format!(
                "source clip `{}` has no server-reported duration; refusing Remaster",
                source.id
            )));
        }
    }

    let remaster = source
        .action_config
        .as_ref()
        .and_then(|config| config.action("remaster"));
    if !remaster
        .is_some_and(|action| action.visible == Some(true) && action.disabled == Some(false))
    {
        return Err(CliError::Config(format!(
            "source clip `{}` does not expose an enabled Remaster action",
            source.id
        )));
    }
    Ok(())
}

fn ambiguous_remaster(
    operation_id: &str,
    clip_id: &str,
    model_name: &str,
    variation_category: Option<RemasterVariation>,
    stage: &'static str,
    cause_code: &'static str,
    cause_message: String,
) -> CliError {
    MutationAmbiguity::new(
        format!(
            "Remaster operation {operation_id} lost a reliable response during {stage}; Suno may still have created clips"
        ),
        "remaster",
        operation_id,
        stage,
        cause_code,
        cause_message,
        false,
        "the Remaster endpoint has no client idempotency key, so replay may duplicate clips or credit usage",
        vec![
            "sunox clip list --json".into(),
            "sunox credits --json".into(),
        ],
    )
    .with_context(
        "source_clip_id",
        serde_json::Value::String(clip_id.to_string()),
    )
    .with_context(
        "model_name",
        serde_json::Value::String(model_name.to_string()),
    )
    .with_context("variation_category", serde_json::json!(variation_category))
    .into_error()
}

#[cfg(test)]
mod tests {
    use super::{resolve_style_profile, resolve_variation_category, validate_remaster_source};
    use crate::api::types::{Clip, RemasterStyleProfile, RemasterVariation};

    fn source(overrides: serde_json::Value) -> Clip {
        let mut value = serde_json::json!({
            "id": "clip-a",
            "title": "Source",
            "status": "complete",
            "model_name": "chirp-carp",
            "created_at": "2026-08-24T00:00:00Z",
            "is_trashed": false,
            "metadata": {"duration": 960.0},
            "action_config": {"actions": [{
                "action_type": "remaster",
                "visible": true,
                "disabled": false
            }]}
        });
        let object = value.as_object_mut().expect("clip fixture object");
        for (key, replacement) in overrides.as_object().expect("override object") {
            object.insert(key.clone(), replacement.clone());
        }
        serde_json::from_value(value).expect("clip fixture")
    }

    #[test]
    fn remaster_source_requires_every_server_gate() {
        validate_remaster_source(&source(serde_json::json!({}))).expect("exactly eligible source");

        for (overrides, expected) in [
            (serde_json::json!({"status": "processing"}), "complete"),
            (serde_json::json!({"is_trashed": true}), "trashed"),
            (serde_json::json!({"is_trashed": null}), "trashed"),
            (serde_json::json!({"metadata": {"duration": 960.1}}), "960"),
            (
                serde_json::json!({"metadata": {"duration": -1.0}}),
                "duration",
            ),
            (serde_json::json!({"metadata": {}}), "duration"),
            (
                serde_json::json!({"metadata": {"duration": 10.0, "infill": true}}),
                "infill",
            ),
            (
                serde_json::json!({"action_config": {"actions": [{"action_type": "remaster", "visible": false, "disabled": false}]}}),
                "Remaster action",
            ),
            (
                serde_json::json!({"action_config": {"actions": [{"action_type": "remaster", "visible": true, "disabled": true}]}}),
                "Remaster action",
            ),
            (
                serde_json::json!({"action_config": {"actions": []}}),
                "Remaster action",
            ),
        ] {
            let error = validate_remaster_source(&source(overrides))
                .expect_err("ineligible source must fail closed");
            assert!(
                error.to_string().contains(expected),
                "expected {expected:?} in {error}"
            );
        }
    }

    #[test]
    fn remaster_variation_encoding_is_model_specific() {
        assert_eq!(
            resolve_variation_category("chirp-halibut", None).expect("v6 default"),
            Some(RemasterVariation::Normal)
        );
        assert_eq!(
            resolve_variation_category("chirp-halibut", Some(RemasterVariation::High))
                .expect("v6 explicit variation"),
            Some(RemasterVariation::High)
        );
        assert!(matches!(
            resolve_variation_category("chirp-flounder", None).expect("v5.5 default"),
            Some(RemasterVariation::Normal)
        ));
        assert!(matches!(
            resolve_variation_category("chirp-carp", None).expect("v5 default"),
            Some(RemasterVariation::Normal)
        ));
        assert_eq!(
            resolve_variation_category("chirp-bass", None).expect("v4.5+ omission"),
            None
        );
        let error = resolve_variation_category("chirp-bass", Some(RemasterVariation::High))
            .expect_err("v4.5+ explicit variation must be rejected");
        assert!(error.to_string().contains("--variation"));

        let error = resolve_variation_category("chirp-future", None)
            .expect_err("unknown models must not inherit a guessed variation contract");
        assert!(error.to_string().contains("unsupported Remaster model"));
    }

    #[test]
    fn v6_style_profile_uses_current_web_values_and_default() {
        assert_eq!(
            resolve_style_profile("chirp-halibut", None).expect("v6 default"),
            Some(RemasterStyleProfile::Boost)
        );
        assert_eq!(
            resolve_style_profile("chirp-halibut", Some(RemasterStyleProfile::Natural),)
                .expect("explicit v6 style"),
            Some(RemasterStyleProfile::Natural)
        );
        assert!(
            resolve_style_profile("chirp-flounder", Some(RemasterStyleProfile::Clarity),).is_err()
        );
    }
}
