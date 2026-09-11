use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn command(directory: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("sunox"));
    command.env("XDG_CONFIG_HOME", directory);
    for name in [
        "SUNOX_DEFAULT_MODEL",
        "SUNOX_POLL_INTERVAL_SECS",
        "SUNOX_POLL_TIMEOUT_SECS",
        "SUNOX_OUTPUT_DIR",
        "SUNOX_SERIAL_MUTATIONS",
        "SUNOX_CHALLENGE_BROWSER",
    ] {
        command.env_remove(name);
    }
    command
}

fn write_config(directory: &Path, text: &str) -> PathBuf {
    let config_dir = directory.join("sunox");
    std::fs::create_dir_all(&config_dir).expect("config directory");
    let path = config_dir.join("config.toml");
    std::fs::write(&path, text).expect("invalid config fixture");
    path
}

#[test]
fn config_set_repairs_invalid_persisted_values_without_discarding_other_fields() {
    for stored in ["0", "\"not-an-integer\""] {
        let directory = tempfile::tempdir().expect("isolated config");
        let path = write_config(
            directory.path(),
            &format!(
                "poll_interval_secs = {stored}\noutput_dir = \"/my/songs\"\n[future_setting]\nkeep = \"untouched\"\n"
            ),
        );
        let assertion = command(directory.path())
            .args(["config", "set", "poll_interval_secs", "5", "--json"])
            .assert()
            .success();
        let result: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stdout).expect("result JSON");
        assert_eq!(result["data"]["poll_interval_secs"], 5);
        let stored: toml::Value =
            toml::from_str(&std::fs::read_to_string(path).expect("readback")).expect("stored TOML");
        assert_eq!(stored["poll_interval_secs"].as_integer(), Some(5));
        assert_eq!(stored["output_dir"].as_str(), Some("/my/songs"));
        assert_eq!(stored["future_setting"]["keep"].as_str(), Some("untouched"));
    }
}

#[test]
fn cli_overrides_resolve_bad_file_env_and_earlier_cli_values_before_validation() {
    let directory = tempfile::tempdir().expect("isolated config");
    let path = write_config(
        directory.path(),
        "poll_interval_secs = \"broken-file-type\"\n",
    );
    let assertion = command(directory.path())
        .env("SUNOX_POLL_INTERVAL_SECS", "broken-env-value")
        .args([
            "-c",
            "poll_interval_secs=bad-earlier-cli",
            "-c",
            "poll_interval_secs=7",
            "config",
            "show",
            "--json",
        ])
        .assert()
        .success();
    let result: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("result JSON");
    assert_eq!(result["data"]["poll_interval_secs"], 7);
    assert_eq!(
        std::fs::read_to_string(path).expect("unchanged config"),
        "poll_interval_secs = \"broken-file-type\"\n"
    );
}

#[test]
fn valid_environment_override_repairs_a_lower_priority_persisted_type() {
    let directory = tempfile::tempdir().expect("isolated config");
    write_config(
        directory.path(),
        "serial_mutations = \"broken-file-type\"\n",
    );
    let assertion = command(directory.path())
        .env("SUNOX_SERIAL_MUTATIONS", "false")
        .args(["config", "show", "--json"])
        .assert()
        .success();
    let result: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("result JSON");
    assert_eq!(result["data"]["serial_mutations"], false);
}

#[test]
fn a_successful_single_field_save_reports_remaining_config_errors() {
    let directory = tempfile::tempdir().expect("isolated config");
    let path = write_config(
        directory.path(),
        "poll_interval_secs = 0\npoll_timeout_secs = 0\n",
    );
    let assertion = command(directory.path())
        .env("SUNOX_OUTPUT_DIR", "/from/environment")
        .env("SUNOX_POLL_INTERVAL_SECS", "still-broken-env")
        .args(["config", "set", "poll_interval_secs", "5", "--json"])
        .assert()
        .success();
    let result: serde_json::Value =
        serde_json::from_slice(&assertion.get_output().stdout).expect("result JSON");
    assert_eq!(result["data"]["saved"], true);
    assert_eq!(result["data"]["effective_config"]["ok"], false);
    assert_eq!(result["data"]["effective_config"]["code"], "config_error");
    let stored: toml::Value =
        toml::from_str(&std::fs::read_to_string(path).expect("readback")).expect("stored TOML");
    assert_eq!(stored["poll_interval_secs"].as_integer(), Some(5));
    assert_eq!(stored["poll_timeout_secs"].as_integer(), Some(0));
    assert!(
        stored.get("output_dir").is_none(),
        "environment must not be persisted"
    );
}

#[test]
fn broken_configuration_is_diagnosable_and_never_falls_back_for_business_commands() {
    let directory = tempfile::tempdir().expect("isolated config");
    let path = write_config(directory.path(), "poll_interval_secs = 0\n");
    for args in [
        vec!["doctor", "--json"],
        vec!["config", "check", "--json"],
        vec!["create", "gentle rain", "--read-only", "--json"],
    ] {
        let assertion = command(directory.path()).args(args).assert().code(2);
        let error: serde_json::Value =
            serde_json::from_slice(&assertion.get_output().stderr).expect("error JSON");
        assert_eq!(error["error"]["code"], "config_error");
        assert_eq!(
            error["error"]["details"]["config"]["path"],
            path.to_str().expect("config path")
        );
        assert_eq!(error["error"]["details"]["config"]["ok"], false);
        assert!(
            error["error"]["message"]
                .as_str()
                .expect("message")
                .contains("poll interval")
        );
    }
}

#[test]
fn invalid_target_values_and_unparseable_toml_do_not_change_the_file() {
    for (original, key, value) in [
        (
            "poll_interval_secs = 0\n",
            "poll_interval_secs",
            "still-bad",
        ),
        ("poll_interval_secs = [\n", "poll_interval_secs", "5"),
    ] {
        let directory = tempfile::tempdir().expect("isolated config");
        let path = write_config(directory.path(), original);
        command(directory.path())
            .args(["config", "set", key, value, "--json"])
            .assert()
            .code(2);
        assert_eq!(
            std::fs::read_to_string(path).expect("unchanged config"),
            original
        );
    }
}
