use crate::api::types::{
    AccessibleFeatures, BillingInfo, DownloadCreditPack, DownloadUsage, MaxLengths,
};
use crate::app::AppContext;
use crate::cli::RemasterModel;
use crate::core::CliError;
use crate::output::{self, OutputFormat};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub async fn credits(ctx: &AppContext) -> Result<(), CliError> {
    let info = ctx.client().await?.billing_info().await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(&info),
        OutputFormat::Table => output::table::billing(&info),
    }
    Ok(())
}

pub async fn models(ctx: &AppContext) -> Result<(), CliError> {
    let info = ctx.client().await?.billing_info().await?;
    match ctx.fmt {
        OutputFormat::Json => output::json::success(serde_json::json!({
            "generation": info.models,
            "remaster": info.remaster_model_types,
        })),
        OutputFormat::Table => {
            output::table::models(&info.models);
            output::table::remaster_models(&info.remaster_model_types);
        }
    }
    Ok(())
}

pub async fn capabilities(ctx: &AppContext) -> Result<(), CliError> {
    let info = ctx.client().await?.billing_info().await?;
    let report = capability_report(&info);

    match ctx.fmt {
        OutputFormat::Json => output::json::success(report),
        OutputFormat::Table => {
            println!("Account");
            output::table::billing(&info);
            println!("Generation models");
            output::table::models(&info.models);
            println!("Remaster models");
            output::table::remaster_models(&info.remaster_model_types);

            let feature_rows = feature_rows(&info);
            println!("Account features and CLI coverage");
            output::table::account_features(&feature_rows);

            let limits = safe_account_limits(&info);
            if !limits.is_empty() {
                println!("Account limits");
                output::table::account_limits(&limits);
            }
        }
    }
    Ok(())
}

fn capability_report(info: &BillingInfo) -> Value {
    let features = feature_rows(info)
        .into_iter()
        .map(|(name, sources, status, commands, note)| {
            let sources = sources.split(", ").collect::<Vec<_>>();
            let commands = commands
                .split(", ")
                .filter(|command| !command.is_empty())
                .collect::<Vec<_>>();
            json!({
                "name": name,
                "sources": sources,
                "cli_status": status,
                "commands": commands,
                "note": note,
            })
        })
        .collect::<Vec<_>>();

    let generation_models = info
        .models
        .iter()
        .map(|model| {
            let mut selectors = vec![model.name.clone(), model.external_key.clone()];
            for key in ["id", "model_id"] {
                if let Some(selector) = model.extra.get(key).and_then(Value::as_str) {
                    selectors.push(selector.to_owned());
                }
            }
            selectors.sort();
            selectors.dedup();
            json!({
                "name": model.name,
                "external_key": model.external_key,
                "selectors": selectors,
                "can_use": model.can_use,
                "is_default_model": model.is_default_model,
                "is_default_free_model": model.is_default_free_model,
                "capabilities": model.capabilities,
                "features": model.features,
                "badges": model.badges,
                "max_lengths": safe_model_max_lengths(&model.max_lengths),
            })
        })
        .collect::<Vec<_>>();

    let remaster_models = info
        .remaster_model_types
        .iter()
        .map(|model| {
            let cli_supported = RemasterModel::supports_api_key(&model.external_key);
            let selectors = if cli_supported {
                vec![model.name.clone(), model.external_key.clone()]
            } else {
                Vec::new()
            };
            json!({
                "name": model.name,
                "external_key": model.external_key,
                "selectors": selectors,
                "cli_supported": cli_supported,
                "is_default_model": model.is_default_model,
                "legacy_can_use_diagnostic_only": model.can_use,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "account": {
            "plan_name": info.plan.name,
            "plan_key": info.plan.plan_key,
            "active": info.is_active,
            "period": info.period,
        },
        "generation_models": generation_models,
        "remaster_models": remaster_models,
        "features": features,
        "limits": safe_account_limits(info),
        "downloads": {
            "usage": safe_download_usage_value(info.download_usage.as_ref()),
            "credit_packs": safe_download_credit_packs_value(info.download_credit_packs.as_deref()),
        },
        "protocol_safety": {
            "read_only_mode": "Pass --read-only to reject account-write commands before their first write request. Downloads then require is_download_unlocked=true and never send authorization.",
            "model_selection": "Generation selectors are resolved against current billing data by display name, external key, or account model id; unusable and ambiguous matches fail closed.",
            "remaster": "The plan feature, selected remaster model, source clip state, and source action_config are checked before submission.",
            "downloads": "Only is_download_unlocked=true skips the one-shot authorization POST. MP3/M4A/WAV/mp4 are prepared-first; Stems authorize their parent once. Authorization and download may be plan-metered.",
            "audio_conversion": "Prepared WAV is attempted first. Legacy WAV/OPUS conversion is a separate POST after source unlock and can be forbidden with --no-convert.",
            "ambiguous_writes": "A lost or unusable response after download authorization, generation, Remaster, conversion, Voice creation, Custom Model training/archive, a lyrics-project write, visual generation, or another submitted edit is reported with recovery evidence; it must not be blindly retried.",
        }
    })
}

pub(super) fn safe_download_usage_value(usage: Option<&DownloadUsage>) -> Value {
    usage.map_or(Value::Null, |usage| {
        json!({
            "current_period_downloads_limit": usage.current_period_downloads_limit,
            "current_period_downloads_used": usage.current_period_downloads_used,
            "additional_download_remaining": usage.additional_download_remaining,
        })
    })
}

fn safe_download_credit_packs_value(packs: Option<&[DownloadCreditPack]>) -> Value {
    packs.map_or(Value::Null, |packs| {
        Value::Array(
            packs
                .iter()
                .map(|pack| {
                    json!({
                        "id": pack.id,
                        "amount": pack.amount,
                        "price_amount": pack.price_amount.as_ref(),
                        "price_currency_code": pack.price_currency_code.as_deref(),
                        "price_usd": pack.price_usd.as_ref(),
                    })
                })
                .collect(),
        )
    })
}

type FeatureRow = (String, String, String, String, String);

fn feature_rows(info: &BillingInfo) -> Vec<FeatureRow> {
    let mut features: BTreeMap<String, BTreeSet<&'static str>> = BTreeMap::new();

    if let Some(accessible) = &info.accessible_features {
        collect_accessible_features(accessible, &mut features);
    }

    for feature in &info.plan.usage_plan_features {
        features
            .entry(feature.name.clone())
            .or_default()
            .insert("plan.usage_plan_features");
    }

    features
        .into_iter()
        .map(|(name, sources)| {
            if name == "remaster"
                && !info
                    .remaster_model_types
                    .iter()
                    .any(|model| RemasterModel::supports_api_key(&model.external_key))
            {
                return (
                    name,
                    sources.into_iter().collect::<Vec<_>>().join(", "),
                    "unsupported".to_owned(),
                    "clip actions".to_owned(),
                    "The account exposes Remaster, but only with future model request shapes this CLI does not guess; source-action inspection remains available."
                        .to_owned(),
                );
            }
            let coverage = feature_coverage(&name);
            (
                name,
                sources.into_iter().collect::<Vec<_>>().join(", "),
                coverage.status.to_owned(),
                coverage.commands.join(", "),
                coverage.note.to_owned(),
            )
        })
        .collect()
}

fn collect_accessible_features(
    accessible: &AccessibleFeatures,
    features: &mut BTreeMap<String, BTreeSet<&'static str>>,
) {
    for name in accessible.enabled_names() {
        features
            .entry(name.to_owned())
            .or_default()
            .insert("accessible_features");
    }
}

struct FeatureCoverage {
    status: &'static str,
    commands: &'static [&'static str],
    note: &'static str,
}

fn feature_coverage(name: &str) -> FeatureCoverage {
    let (status, commands, note): (&str, &[&str], &str) = match name {
        "v4" | "auk" => (
            "supported",
            &["create --model"],
            "Selectable when the corresponding live account model is usable.",
        ),
        "cover" => (
            "supported",
            &["clip cover"],
            "Uses the current generation-backed Cover contract.",
        ),
        "negative_tags" => (
            "supported",
            &["create --exclude", "clip extend --exclude"],
            "Validated against the selected model's current max_lengths.",
        ),
        "remaster" => (
            "supported",
            &["clip actions", "clip remaster"],
            "Account, model, source state, and source action availability are preflighted.",
        ),
        "create_control_sliders" => (
            "supported",
            &["create --weirdness", "create --style-influence"],
            "Sent only for models that advertise the current feature.",
        ),
        "playlist_condition" => (
            "supported",
            &["clip inspire"],
            "One-source playlist-conditioned inspiration is implemented.",
        ),
        "tag_upsample" => (
            "supported",
            &["create --enhance-tags", "clip inspire --enhance-tags"],
            "Opt-in; it is never run implicitly.",
        ),
        "convert_audio" => (
            "supported",
            &["download --format wav", "download --format opus"],
            "Prepared WAV is preferred; legacy WAV/OPUS conversion requires an unlocked source, and --no-convert prevents starting it.",
        ),
        "edit_mode" => (
            "partial",
            &[
                "clip extend",
                "clip crop",
                "clip fade",
                "clip speed",
                "clip reverse",
                "lyrics rewrite",
                "lyrics mashup",
                "lyrics mashup-status",
            ],
            "Common non-Studio clip edits plus Lyrics 2.0 selection rewrite (`lyrics-infill`) and lyrics mashup are implemented; Studio/audio underpaint, overpaint, and section replacement are not.",
        ),
        "persona" => (
            "partial",
            &["create --persona", "persona", "voice"],
            "Persona use/management and the current private verified Voice upload workflow are implemented; microphone capture remains external to the CLI.",
        ),
        "get_stems" => (
            "partial",
            &["clip stems", "clip get-stems"],
            "Implements Pro Auto Split, Pro Split from Mix, and read/download of existing stem banks; Premier-only arbitrary Advanced Split instruments remain intentionally gated.",
        ),
        "long_uploads" => (
            "partial",
            &["clip upload"],
            "Supported file types and size are checked locally; account duration entitlement remains server-authoritative.",
        ),
        "custom_models" => (
            "partial",
            &["create --model", "models", "models custom"],
            "Ready-model selection, pending inspection, exact-ID archive, and training are implemented. Training requires --confirm-rights, 6 to 100 distinct eligible source clips, a 1-to-16-character name, the live custom_models entitlement, and --confirm-ui-available after visibly confirming the current Suno Web training UI; without either attestation the CLI sends no training POST, and the server remains authoritative.",
        ),
        "commercial_rights" => (
            "account_only",
            &[],
            "This is an account entitlement, not a CLI operation.",
        ),
        "credit_topups" => (
            "account_only",
            &["credits"],
            "The CLI can read credits but does not purchase top-ups.",
        ),
        "generate_song_image" => (
            "implemented",
            &[
                "clip generate-image",
                "clip cover-art image",
                "clip cover-art apply-image",
            ],
            "Direct prompt-image apply and the current multi-result image batch workflow are implemented with ownership/action checks, dynamic model/cost discovery, bounded recovery, and business readback.",
        ),
        "generate_song_video" => (
            "implemented",
            &[
                "clip cover-art video",
                "clip cover-art status",
                "clip cover-art apply-video",
                "clip video-status",
            ],
            "The current multi-result cover-video batch workflow is implemented. The separate legacy per-clip submit remains fail-closed; its status read is retained.",
        ),
        _ => (
            "unknown",
            &[],
            "Not mapped by this CLI version; treat as unsupported until its protocol is verified.",
        ),
    };
    FeatureCoverage {
        status,
        commands,
        note,
    }
}

fn safe_account_limits(info: &BillingInfo) -> BTreeMap<String, Value> {
    let mut limits = BTreeMap::from([(
        "monthly_credit_limit".to_owned(),
        Value::from(info.monthly_limit),
    )]);
    if let Some(usage) = &info.download_usage {
        limits.insert(
            "downloads.current_period_limit".to_owned(),
            Value::from(usage.current_period_downloads_limit),
        );
        limits.insert(
            "downloads.current_period_used".to_owned(),
            Value::from(usage.current_period_downloads_used),
        );
        limits.insert(
            "downloads.additional_remaining".to_owned(),
            Value::from(usage.additional_download_remaining),
        );
    }
    if let Some(packs) = &info.download_credit_packs {
        limits.insert(
            "downloads.credit_pack_count".to_owned(),
            Value::from(packs.len() as u64),
        );
    }
    collect_safe_limits("billing", &info.extra, &mut limits);
    collect_safe_limits("plan", &info.plan.extra, &mut limits);
    limits
}

fn safe_model_max_lengths(max_lengths: &MaxLengths) -> BTreeMap<String, Value> {
    let mut limits = BTreeMap::from([
        ("title".to_owned(), Value::from(max_lengths.title)),
        ("prompt".to_owned(), Value::from(max_lengths.prompt)),
        ("tags".to_owned(), Value::from(max_lengths.tags)),
        (
            "negative_tags".to_owned(),
            Value::from(max_lengths.negative_tags),
        ),
        (
            "gpt_description_prompt".to_owned(),
            Value::from(max_lengths.gpt_description_prompt),
        ),
    ]);
    for (key, value) in &max_lengths.extra {
        if let Some(value) = sanitize_metric_value(key, value) {
            limits.insert(key.clone(), value);
        }
    }
    limits
}

fn collect_safe_limits(
    scope: &str,
    source: &BTreeMap<String, Value>,
    destination: &mut BTreeMap<String, Value>,
) {
    for (key, value) in source {
        if is_safe_limit_root(key)
            && let Some(value) = sanitize_metric_value(key, value)
        {
            destination.insert(format!("{scope}.{key}"), value);
        }
    }
}

fn sanitize_metric_value(field_name: &str, value: &Value) -> Option<Value> {
    if is_sensitive_metric_key(field_name) {
        return None;
    }

    match value {
        Value::Bool(_) | Value::Number(_) if is_safe_metric_leaf(field_name) => Some(value.clone()),
        Value::Object(fields) => {
            let fields = fields
                .iter()
                .filter_map(|(key, value)| {
                    sanitize_metric_value(key, value).map(|value| (key.clone(), value))
                })
                .collect::<serde_json::Map<_, _>>();
            (!fields.is_empty()).then_some(Value::Object(fields))
        }
        Value::Array(values) => {
            let values = values
                .iter()
                .filter_map(|value| sanitize_metric_value(field_name, value))
                .collect::<Vec<_>>();
            (!values.is_empty()).then_some(Value::Array(values))
        }
        Value::Null | Value::String(_) | Value::Bool(_) | Value::Number(_) => None,
    }
}

fn is_safe_limit_root(key: &str) -> bool {
    if is_sensitive_metric_key(key) {
        return false;
    }
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "audio_upload_limits" | "voice_limits" | "agentic_limits"
    ) || key.ends_with("_limit")
        || key.ends_with("_limits")
        || key.ends_with("_quota")
        || key.ends_with("_quotas")
}

fn is_safe_metric_leaf(key: &str) -> bool {
    if is_sensitive_metric_key(key) {
        return false;
    }
    let key = key.to_ascii_lowercase();
    matches!(
        key.as_str(),
        "min"
            | "max"
            | "remaining"
            | "used"
            | "available"
            | "enabled"
            | "allowed"
            | "unlimited"
            | "count"
            | "total"
            | "limit"
            | "limits"
            | "quota"
            | "quotas"
            | "duration"
    ) || [
        "min_",
        "max_",
        "remaining_",
        "used_",
        "available_",
        "allowed_",
    ]
    .iter()
    .any(|prefix| key.starts_with(prefix))
        || [
            "_min",
            "_max",
            "_remaining",
            "_used",
            "_available",
            "_enabled",
            "_allowed",
            "_unlimited",
            "_count",
            "_total",
            "_limit",
            "_limits",
            "_quota",
            "_quotas",
            "_seconds",
            "_secs",
            "_minutes",
            "_hours",
            "_days",
            "_bytes",
            "_kb",
            "_mb",
            "_gb",
            "_s",
        ]
        .iter()
        .any(|suffix| key.ends_with(suffix))
}

fn is_sensitive_metric_key(key: &str) -> bool {
    let compact = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    [
        "token",
        "secret",
        "email",
        "jwt",
        "cookie",
        "apikey",
        "password",
        "authorization",
        "credential",
        "bearer",
        "identifier",
    ]
    .iter()
    .any(|sensitive| compact.contains(sensitive))
        || compact == "id"
        || compact.ends_with("id")
}

#[cfg(test)]
mod tests {
    use super::{capability_report, feature_rows, safe_account_limits};
    use crate::api::types::BillingInfo;

    fn billing_fixture() -> BillingInfo {
        serde_json::from_value(serde_json::json!({
            "credits": 42,
            "total_credits_left": 42,
            "monthly_usage": 8,
            "monthly_limit": 2500,
            "is_active": true,
            "plan": {
                "name": "Pro",
                "plan_key": "pro",
                "usage_plan_features": [
                    {"name": "remaster"},
                    {"name": "future_feature"}
                ],
                "voice_limits": {"max": 10},
                "subscriber_id": "must-not-leak"
            },
            "accessible_features": ["remaster", "convert_audio"],
            "models": [{
                "id": "account-model-id",
                "name": "v5.5",
                "external_key": "chirp-fenix",
                "can_use": true,
                "is_default_model": true,
                "description": "fixture",
                "max_lengths": {
                    "duration": 480,
                    "nested": {
                        "remaining": 2,
                        "api_key": "must-not-leak-model-key"
                    },
                    "label": "must-not-leak-model-label"
                }
            }],
            "period": "monthly",
            "renews_on": null,
            "download_usage": {
                "current_period_downloads_limit": 20,
                "current_period_downloads_used": 4,
                "additional_download_remaining": 2,
                "session_token": "must-not-leak-download-usage"
            },
            "download_credit_packs": [{
                "id": "pack-100",
                "amount": 100,
                "price_amount": 999,
                "price_currency_code": "USD",
                "customer_token": "must-not-leak-download-pack"
            }],
            "remaster_model_types": [{
                "name": "v5.5",
                "external_key": "chirp-flounder",
                "is_default_model": true,
                "can_use": false
            }],
            "audio_upload_limits": {
                "min_duration_s": 6,
                "max_duration_s": 1800,
                "nested": {
                    "session_token": "must-not-leak-token",
                    "id": 987654321,
                    "opaque_id": {"max": 123},
                    "jwt": "must-not-leak-jwt",
                    "cookie": "must-not-leak-cookie",
                    "api_key": "must-not-leak-api-key",
                    "password": "must-not-leak-password",
                    "authorization": "must-not-leak-authorization",
                    "credential": "must-not-leak-credential",
                    "note": "must-not-leak-string",
                    "remaining": 3,
                    "enabled": true
                },
                "windows": [
                    {"max": 4, "credential": "must-not-leak-array-credential"},
                    "must-not-leak-array-string"
                ]
            },
            "session_token": "must-not-leak"
        }))
        .expect("valid billing fixture")
    }

    #[test]
    fn capability_report_is_account_driven_and_sanitized() {
        let report = capability_report(&billing_fixture());
        assert_eq!(report["account"]["plan_key"], "pro");
        assert_eq!(
            report["generation_models"][0]["selectors"][0],
            "account-model-id"
        );
        assert_eq!(
            report["remaster_models"][0]["legacy_can_use_diagnostic_only"],
            false
        );
        assert_eq!(report["remaster_models"][0]["cli_supported"], true);
        assert_eq!(
            report["remaster_models"][0]["selectors"][1],
            "chirp-flounder"
        );
        assert!(
            report["protocol_safety"]["ambiguous_writes"]
                .as_str()
                .is_some_and(|message| message.contains("download authorization")
                    && message.contains("conversion")
                    && message.contains("edit"))
        );
        assert!(
            report["protocol_safety"]["downloads"]
                .as_str()
                .is_some_and(|message| message.contains("is_download_unlocked=true")
                    && message.contains("parent once"))
        );
        assert_eq!(
            report["downloads"]["usage"]["current_period_downloads_limit"],
            20
        );
        assert_eq!(report["downloads"]["credit_packs"][0]["id"], "pack-100");
        assert!(report.to_string().contains("audio_upload_limits"));
        assert!(!report.to_string().contains("must-not-leak"));
        assert_eq!(
            report["generation_models"][0]["max_lengths"]["duration"],
            480
        );
        assert_eq!(
            report["generation_models"][0]["max_lengths"]["nested"]["remaining"],
            2
        );
        assert!(
            report["generation_models"][0]["max_lengths"]["nested"]
                .get("api_key")
                .is_none()
        );
        assert!(
            report["generation_models"][0]["max_lengths"]
                .get("label")
                .is_none()
        );
    }

    #[test]
    fn feature_rows_merge_current_and_plan_sources_without_overclaiming_unknowns() {
        let mut fixture = billing_fixture();
        fixture.accessible_features = serde_json::from_value(serde_json::json!([
            "remaster",
            "convert_audio",
            {"name": "disabled_future", "enabled": false}
        ]))
        .expect("typed accessible features");
        let rows = feature_rows(&fixture);
        let remaster = rows
            .iter()
            .find(|row| row.0 == "remaster")
            .expect("remaster row");
        assert_eq!(remaster.2, "supported");
        assert!(remaster.1.contains("accessible_features"));
        assert!(remaster.1.contains("plan.usage_plan_features"));

        let future = rows
            .iter()
            .find(|row| row.0 == "future_feature")
            .expect("future feature row");
        assert_eq!(future.2, "unknown");
        assert!(rows.iter().all(|row| row.0 != "disabled_future"));
    }

    #[test]
    fn future_only_remaster_models_are_reported_but_not_advertised_as_selectors() {
        let mut fixture = billing_fixture();
        fixture.remaster_model_types[0].name = "vNext".into();
        fixture.remaster_model_types[0].external_key = "chirp-future".into();

        let report = capability_report(&fixture);
        assert_eq!(report["remaster_models"][0]["cli_supported"], false);
        assert_eq!(
            report["remaster_models"][0]["selectors"]
                .as_array()
                .map(Vec::len),
            Some(0)
        );

        let rows = feature_rows(&fixture);
        let remaster = rows
            .iter()
            .find(|row| row.0 == "remaster")
            .expect("remaster feature row");
        assert_eq!(remaster.2, "unsupported");
        assert!(remaster.4.contains("does not guess"));
    }

    #[test]
    fn safe_limits_keep_only_recursive_numeric_and_boolean_metrics() {
        let limits = safe_account_limits(&billing_fixture());
        assert_eq!(limits["downloads.current_period_limit"], 20);
        assert_eq!(limits["downloads.current_period_used"], 4);
        assert_eq!(limits["downloads.additional_remaining"], 2);
        assert_eq!(limits["downloads.credit_pack_count"], 1);
        assert!(limits.contains_key("billing.audio_upload_limits"));
        assert!(limits.contains_key("plan.voice_limits"));
        assert!(!limits.keys().any(|key| key.contains("token")));
        assert!(!limits.keys().any(|key| key.ends_with("_id")));
        let serialized = serde_json::to_string(&limits).expect("serialize safe limits");
        assert!(!serialized.contains("must-not-leak"));
        assert_eq!(
            limits["billing.audio_upload_limits"]["nested"]["remaining"],
            3
        );
        assert_eq!(
            limits["billing.audio_upload_limits"]["nested"]["enabled"],
            true
        );
        assert_eq!(
            limits["billing.audio_upload_limits"]["windows"][0]["max"],
            4
        );
        assert_eq!(
            limits["billing.audio_upload_limits"]["windows"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        for forbidden in [
            "session_token",
            "id",
            "opaque_id",
            "jwt",
            "cookie",
            "api_key",
            "password",
            "authorization",
            "credential",
            "note",
        ] {
            assert!(
                limits["billing.audio_upload_limits"]["nested"]
                    .get(forbidden)
                    .is_none(),
                "sensitive or non-metric field {forbidden} leaked"
            );
        }
    }
}
