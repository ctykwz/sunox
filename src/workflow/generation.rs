use crate::api::types::ControlSliders;
use crate::cli::VocalGender;
use crate::core::{CliError, ensure_percentage};

pub fn build_tags(tags: Option<&str>, vocal: Option<&VocalGender>) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if let Some(t) = tags {
        parts.push(t);
    }
    match vocal {
        Some(VocalGender::Male) => parts.push("male vocals"),
        Some(VocalGender::Female) => parts.push("female vocals"),
        None => {}
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

/// Build a control_sliders block when --weirdness or --style-influence is set.
/// Returns None when neither is provided so the optional schema field is omitted.
pub fn build_control_sliders(
    weirdness: Option<f64>,
    style_influence: Option<f64>,
) -> Result<Option<ControlSliders>, CliError> {
    if weirdness.is_none() && style_influence.is_none() {
        return Ok(None);
    }
    if let Some(weirdness) = weirdness {
        ensure_percentage("--weirdness", weirdness)?;
    }
    if let Some(style_influence) = style_influence {
        ensure_percentage("--style-influence", style_influence)?;
    }
    Ok(Some(ControlSliders {
        // Normalize 0-100 to 0.0-1.0.
        weirdness_constraint: weirdness.map(|w| w / 100.0),
        style_weight: style_influence.map(|s| s / 100.0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_tags_appends_vocal_direction() {
        let tags = build_tags(Some("indie pop, bright"), Some(&VocalGender::Female));
        assert_eq!(tags.as_deref(), Some("indie pop, bright, female vocals"));
    }

    #[test]
    fn build_tags_returns_none_when_no_inputs_are_present() {
        assert_eq!(build_tags(None, None), None);
    }

    #[test]
    fn build_control_sliders_normalizes_percentages() {
        let sliders = build_control_sliders(Some(25.0), Some(80.0))
            .expect("valid sliders")
            .expect("sliders");
        assert_eq!(sliders.weirdness_constraint, Some(0.25));
        assert_eq!(sliders.style_weight, Some(0.8));
    }

    #[test]
    fn build_control_sliders_rejects_values_outside_the_documented_range() {
        build_control_sliders(Some(150.0), Some(-10.0)).expect_err("invalid sliders");
    }
}
