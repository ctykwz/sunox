use super::CliError;

pub fn ensure_clip_ids(ids: &[String]) -> Result<(), CliError> {
    if ids.is_empty() {
        return Err(CliError::Config("no clip IDs provided".into()));
    }
    Ok(())
}

pub fn ensure_destructive_confirmed(yes: bool, command: &str) -> Result<(), CliError> {
    if !yes {
        return Err(CliError::Config(format!(
            "`{command}` requires -y/--yes because it modifies or removes Suno resources"
        )));
    }
    Ok(())
}

pub fn ensure_percentage(name: &str, value: f64) -> Result<(), CliError> {
    if !value.is_finite() || !(0.0..=100.0).contains(&value) {
        return Err(CliError::Config(format!(
            "{name} must be a finite number between 0 and 100"
        )));
    }
    Ok(())
}

pub fn ensure_non_negative_finite(name: &str, value: f64) -> Result<(), CliError> {
    if !value.is_finite() || value < 0.0 {
        return Err(CliError::Config(format!(
            "{name} must be a finite non-negative number"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_non_negative_finite, ensure_percentage};

    #[test]
    fn percentages_reject_non_finite_and_out_of_range_values() {
        for value in [f64::NAN, f64::INFINITY, -1.0, 101.0] {
            ensure_percentage("--weirdness", value).expect_err("invalid percentage");
        }
        ensure_percentage("--weirdness", 0.0).expect("lower bound");
        ensure_percentage("--weirdness", 100.0).expect("upper bound");
    }

    #[test]
    fn timestamps_reject_non_finite_and_negative_values() {
        for value in [f64::NAN, f64::INFINITY, -0.1] {
            ensure_non_negative_finite("--at", value).expect_err("invalid timestamp");
        }
        ensure_non_negative_finite("--at", 0.0).expect("zero timestamp");
    }
}
