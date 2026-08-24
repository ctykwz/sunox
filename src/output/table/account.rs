use std::collections::BTreeMap;

use crate::api::types::{BillingInfo, Model, RemasterModelInfo};
use serde_json::Value;

use super::{base_table, dynamic_table};

pub fn billing(info: &BillingInfo) {
    let mut table = base_table();
    table.set_header(vec!["Field", "Value"]);

    table.add_row(vec!["Plan", &info.plan.name]);
    table.add_row(vec!["Credits Left", &info.total_credits_left.to_string()]);
    table.add_row(vec![
        "Monthly Usage",
        &format!("{} / {}", info.monthly_usage, info.monthly_limit),
    ]);
    table.add_row(vec!["Active", &info.is_active.to_string()]);
    table.add_row(vec!["Period", &info.period]);
    if let Some(ref renew) = info.renews_on {
        table.add_row(vec!["Renews On", renew]);
    }
    println!("{table}");
}

pub fn models(models: &[Model]) {
    let mut table = base_table();
    table.set_header(vec![
        "Name",
        "Key",
        "Default",
        "Max Prompt",
        "Max Tags",
        "Description",
    ]);

    for model in models {
        if !model.can_use {
            continue;
        }
        table.add_row(vec![
            &model.name,
            &model.external_key,
            &if model.is_default_model {
                "yes".into()
            } else {
                String::new()
            },
            &model.max_lengths.prompt.to_string(),
            &model.max_lengths.tags.to_string(),
            &model.description,
        ]);
    }
    println!("{table}");
}

pub fn remaster_models(models: &[RemasterModelInfo]) {
    let mut table = base_table();
    table.set_header(vec!["Remaster", "Key", "CLI", "Web order", "Default flag"]);
    for (index, model) in models.iter().enumerate() {
        table.add_row(vec![
            model.name.clone(),
            model.external_key.clone(),
            if crate::cli::RemasterModel::supports_api_key(&model.external_key) {
                "supported".to_string()
            } else {
                "unsupported".to_string()
            },
            (index + 1).to_string(),
            if model.is_default_model {
                "yes".to_string()
            } else {
                String::new()
            },
        ]);
    }
    println!("{table}");
}

pub fn account_features(rows: &[(String, String, String, String, String)]) {
    let mut table = dynamic_table();
    table.set_header(vec!["Feature", "Source", "CLI", "Commands", "Note"]);
    for (name, source, status, commands, note) in rows {
        table.add_row(vec![name, source, status, commands, note]);
    }
    println!("{table}");
}

pub fn account_limits(limits: &BTreeMap<String, Value>) {
    let mut table = dynamic_table();
    table.set_header(vec!["Limit", "Value"]);
    for (name, value) in limits {
        let rendered = serde_json::to_string(value).unwrap_or_else(|_| "<unavailable>".into());
        table.add_row(vec![name, &rendered]);
    }
    println!("{table}");
}
