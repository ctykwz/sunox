use serde_json::Value;

use super::SunoClient;
use super::types::{BillingInfo, Clip, GenerateRequest};
use crate::core::CliError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintMode {
    Underpaint,
    Overpaint,
}

impl PaintMode {
    fn task(self) -> &'static str {
        match self {
            Self::Underpaint => "underpainting",
            Self::Overpaint => "overpainting",
        }
    }

    fn action_label(self) -> &'static str {
        match self {
            Self::Underpaint => "Add Instrumental",
            Self::Overpaint => "Add Vocal",
        }
    }
}

pub struct PaintOptions<'a> {
    pub clip_id: &'a str,
    pub title: Option<&'a str>,
    pub lyrics: Option<&'a str>,
    pub tags: Option<&'a str>,
    pub negative_tags: Option<&'a str>,
    pub model: &'a str,
    pub mode: PaintMode,
}

impl SunoClient {
    pub(crate) async fn prepare_paint_request(
        &self,
        options: PaintOptions<'_>,
    ) -> Result<GenerateRequest, CliError> {
        let clip_id = options.clip_id.trim();
        if clip_id.is_empty() {
            return Err(CliError::Config("source clip ID must not be empty".into()));
        }

        let billing = self.billing_info().await?;
        ensure_edit_mode_access(&billing)?;
        let source = self
            .get_clip(clip_id)
            .await?
            .ok_or_else(|| CliError::NotFound(format!("clip: {clip_id}")))?;
        validate_paint_source(
            &source,
            clip_id,
            self.authenticated_user_id().as_deref(),
            options.mode,
        )?;

        let mut req = GenerateRequest::new(options.model, "custom");
        req.task = Some(options.mode.task().into());
        req.title = Some(
            options
                .title
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{} ({})", source.title, options.mode.action_label())),
        );
        req.prompt = options
            .lyrics
            .map(str::to_string)
            .or(source.metadata.prompt)
            .unwrap_or_default();
        req.tags = Some(
            options
                .tags
                .map(str::to_string)
                .or(source.metadata.tags)
                .unwrap_or_default(),
        );
        req.negative_tags = options
            .negative_tags
            .map(str::to_string)
            .or(source.metadata.negative_tags)
            .unwrap_or_default();
        match options.mode {
            PaintMode::Underpaint => req.underpainting_clip_id = Some(clip_id.to_string()),
            PaintMode::Overpaint => req.overpainting_clip_id = Some(clip_id.to_string()),
        }
        req.metadata.is_remix = Some(true);
        Ok(req)
    }
}

fn ensure_edit_mode_access(info: &BillingInfo) -> Result<(), CliError> {
    let enabled = info
        .accessible_features
        .as_ref()
        .is_some_and(|features| features.contains("edit_mode"))
        || info
            .plan
            .usage_plan_features
            .iter()
            .any(|feature| feature.name == "edit_mode");
    if enabled {
        Ok(())
    } else {
        Err(CliError::Config(
            "the current Suno account does not advertise the edit_mode entitlement required for Underpaint/Overpaint"
                .into(),
        ))
    }
}

fn validate_paint_source(
    source: &Clip,
    requested_id: &str,
    authenticated_user_id: Option<&str>,
    mode: PaintMode,
) -> Result<(), CliError> {
    if source.id != requested_id {
        return Err(CliError::Api {
            code: "schema_drift",
            message: format!(
                "paint source lookup for `{requested_id}` returned clip `{}`",
                source.id
            ),
        });
    }
    let authenticated_user_id = authenticated_user_id.ok_or_else(|| {
        CliError::Config(
            "the authenticated account identity is not present in the current JWT; refusing Underpaint/Overpaint"
                .into(),
        )
    })?;
    let source_user_id = source.extra.get("user_id").and_then(Value::as_str);
    if source_user_id != Some(authenticated_user_id) {
        return Err(CliError::Config(format!(
            "{} source clip `{requested_id}` must be explicitly owned by the authenticated account",
            mode.action_label()
        )));
    }
    if source.status != "complete" {
        return Err(CliError::Config(format!(
            "{} source clip `{requested_id}` must be complete",
            mode.action_label()
        )));
    }
    if source.is_trashed != Some(false) {
        return Err(CliError::Config(format!(
            "{} source clip `{requested_id}` must be explicitly non-trashed",
            mode.action_label()
        )));
    }

    let stem_group = source
        .metadata
        .extra
        .get("stem_type_group_name")
        .and_then(Value::as_str);
    let is_upload = source.metadata.clip_type.as_deref() == Some("upload");
    let eligible = match mode {
        PaintMode::Underpaint => {
            is_upload || matches!(stem_group, Some("Vocals" | "Backing_Vocals"))
        }
        PaintMode::Overpaint => {
            let prompt = source.metadata.prompt.as_deref().unwrap_or("").trim();
            is_upload
                || stem_group == Some("Instrumental")
                || prompt.is_empty()
                || (prompt.starts_with('[')
                    && prompt.ends_with(']')
                    && !prompt.contains(['\n', '\r']))
        }
    };
    if eligible {
        Ok(())
    } else {
        Err(CliError::Config(format!(
            "source clip `{requested_id}` is not eligible for {} under the current Suno Web source rules",
            mode.action_label()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::{PaintMode, validate_paint_source};
    use crate::api::types::Clip;

    fn clip(prompt: &str, clip_type: &str, stem_group: Option<&str>) -> Clip {
        let mut value = serde_json::json!({
            "id": "clip-1",
            "title": "Source",
            "status": "complete",
            "model_name": "chirp-hawk",
            "created_at": "2026-09-11T00:00:00Z",
            "is_trashed": false,
            "user_id": "user-1",
            "metadata": {"prompt": prompt, "type": clip_type}
        });
        if let Some(stem_group) = stem_group {
            value["metadata"]["stem_type_group_name"] = serde_json::json!(stem_group);
        }
        serde_json::from_value(value).expect("clip fixture")
    }

    #[test]
    fn source_eligibility_matches_current_web_rules() {
        let vocal = clip("lyrics", "gen", Some("Vocals"));
        validate_paint_source(&vocal, "clip-1", Some("user-1"), PaintMode::Underpaint)
            .expect("vocal stem can receive instrumental");

        let instrumental = clip("[Instrumental]", "gen", None);
        validate_paint_source(
            &instrumental,
            "clip-1",
            Some("user-1"),
            PaintMode::Overpaint,
        )
        .expect("instrumental prompt can receive vocals");

        let song = clip("ordinary lyrics", "gen", None);
        assert!(
            validate_paint_source(&song, "clip-1", Some("user-1"), PaintMode::Overpaint).is_err()
        );
    }
}
