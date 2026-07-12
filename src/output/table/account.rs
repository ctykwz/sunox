use crate::api::types::{BillingInfo, Model, RemasterModelInfo};

use super::base_table;

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
    table.set_header(vec!["Remaster", "Key", "Default", "Availability"]);
    for model in models {
        let availability = match model.can_use {
            Some(true) => "available",
            Some(false) => "unavailable",
            None => "not reported",
        };
        table.add_row(vec![
            model.name.clone(),
            model.external_key.clone(),
            if model.is_default_model {
                "yes".to_string()
            } else {
                String::new()
            },
            availability.to_string(),
        ]);
    }
    println!("{table}");
}
