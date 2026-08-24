use serde_json::json;

use super::SunoClient;
use super::types::{CowriteLyricsModel, CowriteLyricsResponse};
use crate::core::CliError;

pub struct CowriteLyricsOptions<'a> {
    pub prompt: &'a str,
    pub model: Option<&'a str>,
    pub enable_thinking: bool,
}

impl SunoClient {
    pub async fn cowrite_lyrics_models(&self) -> Result<Vec<CowriteLyricsModel>, CliError> {
        self.with_auth_retry(|| async {
            self.read_json_with_transport_retry(self.get("/api/generate/cowrite-lyrics/models/"))
                .await
        })
        .await
    }

    /// Generate fresh lyrics through the current Web Cowrite submit contract.
    pub async fn generate_lyrics(
        &self,
        options: CowriteLyricsOptions<'_>,
    ) -> Result<CowriteLyricsResponse, CliError> {
        let models = self.cowrite_lyrics_models().await?;
        let model = select_cowrite_model(&models, options.model)?;
        if options.enable_thinking && !model.supports_thinking {
            return Err(CliError::Config(format!(
                "Suno Cowrite model `{}` does not support thinking",
                model.id
            )));
        }
        self.with_auth_retry(|| async {
            let resp = self
                .post("/api/generate/cowrite-lyrics/")
                .json(&json!({
                    "selected": "",
                    "context_before": "",
                    "context_after": "",
                    "instruction": options.prompt,
                    "title": "",
                    "style": "",
                    "mode": "apply_user_request",
                    "references": [],
                    "num_variants": null,
                    "lyricist_id": null,
                    "metadata": {
                        "lyrics_model": model.id,
                        "enable_thinking": options.enable_thinking
                    },
                    "create_session_token": null,
                    "lyrics_project_id": null
                }))
                .send()
                .await?;
            let resp = self.check_response(resp).await?;
            Ok(resp.json().await?)
        })
        .await
    }
}

struct SelectedCowriteModel {
    id: String,
    supports_thinking: bool,
}

fn select_cowrite_model(
    models: &[CowriteLyricsModel],
    requested: Option<&str>,
) -> Result<SelectedCowriteModel, CliError> {
    if let Some(requested) = requested {
        let requested = requested.trim();
        return models
            .iter()
            .find(|model| {
                model.id.eq_ignore_ascii_case(requested)
                    || model.display_name.eq_ignore_ascii_case(requested)
            })
            .map(|model| SelectedCowriteModel {
                id: model.id.clone(),
                supports_thinking: model.supports_thinking,
            })
            .ok_or_else(|| {
                CliError::Config(format!("unknown Suno Cowrite lyrics model `{requested}`"))
            });
    }

    Ok(SelectedCowriteModel {
        id: "default".into(),
        supports_thinking: models
            .iter()
            .find(|model| model.id.eq_ignore_ascii_case("default"))
            .is_some_and(|model| model.supports_thinking),
    })
}
