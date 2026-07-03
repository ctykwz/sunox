use assert_cmd::Command;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

fn isolated_test_home(prefix: &str) -> PathBuf {
    let test_home = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_dir_all(&test_home);
    std::fs::create_dir_all(&test_home).expect("test home");
    test_home
}

fn with_isolated_home<'a>(cmd: &'a mut Command, test_home: &Path) -> &'a mut Command {
    cmd.env("HOME", test_home)
        .env("XDG_CONFIG_HOME", test_home.join(".config"))
        .env("XDG_DATA_HOME", test_home.join(".local").join("share"))
        .env_remove("SUNO_DEFAULT_MODEL")
        .env_remove("SUNO_POLL_INTERVAL_SECS")
        .env_remove("SUNO_POLL_TIMEOUT_SECS")
        .env_remove("SUNO_OUTPUT_DIR")
        .env_remove("SUNO_SERIAL_MUTATIONS")
}

#[test]
fn help_lists_codex_style_commands() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage: sunox [OPTIONS] [PROMPT]"))
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("download"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("clip"))
        .stdout(predicate::str::contains("login"))
        .stdout(predicate::str::contains("logout"))
        .stdout(predicate::str::contains("doctor"))
        .stdout(predicate::str::contains("-c, --config <key=value>"))
        .stdout(predicate::str::contains("--parallel"))
        .stdout(predicate::str::contains("generate").not());
}

#[test]
fn create_help_accepts_prompt_argument() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: sunox create [OPTIONS] [PROMPT]",
        ))
        .stdout(predicate::str::contains("--title"))
        .stdout(predicate::str::contains("--tags"))
        .stdout(predicate::str::contains("--captcha"));
}

#[test]
fn clip_help_groups_clip_subcommands() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Manage clips"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("download"))
        .stdout(predicate::str::contains("upload"))
        .stdout(predicate::str::contains("timed-lyrics"));
}

#[test]
fn clip_list_help_exposes_web_feed_filters() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--liked"))
        .stdout(predicate::str::contains("--public"))
        .stdout(predicate::str::contains("--upload"))
        .stdout(predicate::str::contains("--cover"))
        .stdout(predicate::str::contains("--extend"))
        .stdout(predicate::str::contains("--sort <SORT>"));
}

#[test]
fn clip_status_reuses_existing_validation() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "status", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("no clip IDs provided"));
}

#[test]
fn login_logout_and_doctor_help_are_available() {
    for command in ["login", "logout", "doctor"] {
        let mut cmd = Command::cargo_bin("sunox").expect("binary");

        cmd.args([command, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Usage:"));
    }
}

#[test]
fn help_lists_playlist_command() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("playlist"))
        .stdout(predicate::str::contains("Manage playlists"));
}

#[test]
fn help_lists_clip_management_commands() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("restore"))
        .stdout(predicate::str::contains("like"))
        .stdout(predicate::str::contains("dislike"));
}

#[test]
fn help_lists_upload_command() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("upload"))
        .stdout(predicate::str::contains("Upload a local audio file"));
}

#[test]
fn upload_help_lists_workflow_flags() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "upload", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("<FILE>"))
        .stdout(predicate::str::contains("--upload-type"))
        .stdout(predicate::str::contains("--stem-mix"))
        .stdout(predicate::str::contains("--title"))
        .stdout(predicate::str::contains("--lyrics-file"))
        .stdout(predicate::str::contains("--timeout"));
}

#[test]
fn playlist_help_lists_management_subcommands() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["playlist", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("publish"))
        .stdout(predicate::str::contains("reorder"))
        .stdout(predicate::str::contains("restore"))
        .stdout(predicate::str::contains("save"))
        .stdout(predicate::str::contains("unsave"))
        .stdout(predicate::str::contains("like"))
        .stdout(predicate::str::contains("dislike"))
        .stdout(predicate::str::contains("delete"));
}

#[test]
fn playlist_set_help_lists_image_url() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["playlist", "set", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--image-url"))
        .stdout(predicate::str::contains("--image-file"));
}

#[test]
fn clip_set_help_lists_cover_options() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "set", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--image-url"))
        .stdout(predicate::str::contains("--image-file"))
        .stdout(predicate::str::contains("--remove-video-cover"));
}

#[test]
fn clip_set_rejects_multiple_cover_sources() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args([
        "clip",
        "set",
        "clip-a",
        "--image-url",
        "https://cdn2.suno.ai/image_a.jpeg",
        "--image-file",
        "cover.png",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn persona_help_lists_management_subcommands() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["persona", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("clips"))
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("processed-clip"))
        .stdout(predicate::str::contains("publish"))
        .stdout(predicate::str::contains("unpublish"))
        .stdout(predicate::str::contains("love"))
        .stdout(predicate::str::contains("unlove"))
        .stdout(predicate::str::contains("toggle-love"))
        .stdout(predicate::str::contains("delete"))
        .stdout(predicate::str::contains("restore"))
        .stdout(predicate::str::contains("purge"));
}

#[test]
fn create_rejects_removed_wait_flag() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args([
        "create",
        "--title",
        "Test",
        "--tags",
        "pop",
        "--lyrics",
        "[Verse]\nHello",
        "--wait",
        "--no-captcha",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("unexpected argument '--wait'"));
}

#[test]
fn create_rejects_removed_download_flag() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args([
        "create",
        "--title",
        "Test",
        "--tags",
        "pop",
        "--lyrics",
        "[Verse]\nHello",
        "--download",
        "./out",
        "--no-captcha",
    ])
    .assert()
    .failure()
    .stderr(predicate::str::contains("unexpected argument '--download'"));
}

#[test]
fn create_help_lists_optional_captcha_flag() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["create", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--captcha"))
        .stdout(predicate::str::contains("[default: v5.5]").not())
        .stdout(predicate::str::contains("--variation").not());
}

#[test]
fn top_level_help_omits_removed_generate_command() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["--help"]).assert().success().stdout(
        predicate::str::contains("Generate music with custom lyrics, tags, and controls").not(),
    );
}

#[test]
fn cover_help_does_not_hardcode_generation_model_default() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "cover", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--model"))
        .stdout(predicate::str::contains("[default: v5.5]").not());
}

#[test]
fn speed_help_exposes_live_adjust_speed_contract() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "speed", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--multiplier"))
        .stdout(predicate::str::contains("--no-keep-pitch"))
        .stdout(predicate::str::contains("--title"));
}

#[test]
fn generate_backed_clip_commands_expose_challenge_controls() {
    for args in [
        ["clip", "cover", "--help"],
        ["clip", "extend", "--help"],
        ["clip", "stems", "--help"],
    ] {
        let mut cmd = Command::cargo_bin("sunox").expect("binary");
        cmd.args(args)
            .assert()
            .success()
            .stdout(predicate::str::contains("--token"))
            .stdout(predicate::str::contains("--captcha"))
            .stdout(predicate::str::contains("--no-captcha"));
    }
}

#[test]
fn extend_help_exposes_source_defaults_and_instrumental_overrides() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "extend", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--title"))
        .stdout(predicate::str::contains("--exclude"))
        .stdout(predicate::str::contains("--instrumental"))
        .stdout(predicate::str::contains("--no-instrumental"));
}

#[test]
fn install_skill_prints_current_generation_guidance() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["install-skill", "--print"])
        .assert()
        .success()
        .stdout(predicate::str::contains("token=null"))
        .stdout(predicate::str::contains("--captcha"))
        .stdout(predicate::str::contains("sunox create --title"))
        .stdout(predicate::str::contains("returned clip ID"))
        .stdout(predicate::str::contains("do not pass --parallel"))
        .stdout(predicate::str::contains("simple audio analysis"))
        .stdout(predicate::str::contains(
            "current CLI download supports MP3",
        ))
        .stdout(predicate::str::contains("do not publish"))
        .stdout(predicate::str::contains("destructive commands require"))
        .stdout(predicate::str::contains("WAV"))
        .stdout(predicate::str::contains(
            "not the same as Suno Web Pro Get Stems export",
        ))
        .stdout(predicate::str::contains("error.details"))
        .stdout(predicate::str::contains("sunox clip upload <file>"))
        .stdout(predicate::str::contains("sunox clip speed <clip_id>"));
}

#[test]
fn install_skill_defaults_to_codex_skill_directory() {
    let test_home = isolated_test_home("sunox-cli-install-skill-codex-default-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut cmd, &test_home)
        .args(["install-skill", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"target\": \"codex\""))
        .stdout(predicate::str::contains(".codex/skills/sunox/SKILL.md"));

    let installed = test_home.join(".codex/skills/sunox/SKILL.md");
    let skill = std::fs::read_to_string(installed).expect("installed skill");
    assert!(skill.contains("sunox agent-info"));
    assert!(skill.contains("sunox clip wait"));
}

#[test]
fn install_skill_accepts_explicit_codex_target() {
    let test_home = isolated_test_home("sunox-cli-install-skill-codex-explicit-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut cmd, &test_home)
        .args(["install-skill", "--target", "codex", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"target\": \"codex\""))
        .stdout(predicate::str::contains(".codex/skills/sunox/SKILL.md"));
}

#[test]
fn set_rejects_empty_update_before_auth() {
    let test_home = isolated_test_home("sunox-cli-set-before-auth-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["clip", "set", "clip-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains(
            "provide at least one metadata field",
        ));
}

#[test]
fn status_rejects_empty_ids_before_auth() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "status", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("no clip IDs provided"));
}

#[test]
fn download_rejects_empty_ids_before_auth() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["clip", "download", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("no clip IDs provided"));
}

#[test]
fn top_level_download_reuses_clip_download_validation() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["download", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("no clip IDs provided"));
}

#[test]
fn top_level_download_help_is_user_facing() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["download", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: sunox download [OPTIONS] [IDS]...",
        ))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--video"));
}

#[test]
fn top_level_add_requires_clip_ids_before_auth() {
    let test_home = isolated_test_home("sunox-cli-add-before-auth-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["add", "--to", "playlist-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("no clip IDs provided"));
}

#[test]
fn playlist_remove_rejects_empty_ids_before_auth() {
    let test_home = isolated_test_home("sunox-cli-playlist-remove-before-auth-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["playlist", "remove", "playlist-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("no clip IDs provided"));
}

#[test]
fn clip_delete_requires_yes_before_auth() {
    let test_home = isolated_test_home("sunox-cli-clip-delete-yes-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["clip", "delete", "clip-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("requires -y/--yes"));
}

#[test]
fn playlist_delete_requires_yes_before_auth() {
    let test_home = isolated_test_home("sunox-cli-playlist-delete-yes-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["playlist", "delete", "playlist-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("requires -y/--yes"));
}

#[test]
fn persona_purge_requires_yes_before_auth() {
    let test_home = isolated_test_home("sunox-cli-persona-purge-yes-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["persona", "purge", "persona-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("requires -y/--yes"));
}

#[test]
fn playlist_set_rejects_empty_update_before_auth() {
    let test_home = isolated_test_home("sunox-cli-playlist-set-before-auth-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    with_isolated_home(&mut cmd, &test_home)
        .args(["playlist", "set", "playlist-a", "--json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("\"code\": \"config_error\""))
        .stderr(predicate::str::contains("provide at least one"));
}

#[test]
fn top_level_add_help_uses_playlist_language() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Usage: sunox add [OPTIONS] --to <PLAYLIST_ID> [CLIP_IDS]...",
        ))
        .stdout(predicate::str::contains("--to <PLAYLIST_ID>"));
}

#[test]
fn config_set_persists_in_isolated_home() {
    let test_home = isolated_test_home("sunox-cli-config-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut cmd, &test_home)
        .args(["config", "set", "output_dir", "./songs", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"output_dir\": \"./songs\""));

    let mut show = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut show, &test_home)
        .args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""))
        .stdout(predicate::str::contains("\"data\""))
        .stdout(predicate::str::contains("\"output_dir\": \"./songs\""));
}

#[test]
fn config_show_json_uses_success_envelope() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""))
        .stdout(predicate::str::contains("\"data\""))
        .stdout(predicate::str::contains("\"default_model\""))
        .stdout(predicate::str::contains("\"serial_mutations\": true"));
}

#[test]
fn config_show_applies_suno_env_overrides() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.env("SUNO_OUTPUT_DIR", "/tmp/suno-output")
        .env("SUNO_POLL_TIMEOUT_SECS", "777")
        .args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"output_dir\": \"/tmp/suno-output\"",
        ))
        .stdout(predicate::str::contains("\"poll_timeout_secs\": 777"));
}

#[test]
fn config_set_normalizes_default_model_version() {
    let test_home = isolated_test_home("sunox-cli-model-config-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut cmd, &test_home)
        .args(["config", "set", "default_model", "v5.5", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"default_model\": \"chirp-fenix\"",
        ));
}

#[test]
fn global_config_override_applies_without_persisting() {
    let test_home = isolated_test_home("sunox-cli-global-config-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut cmd, &test_home)
        .args([
            "-c",
            "default_model=v5",
            "-c",
            "serial_mutations=false",
            "config",
            "show",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"default_model\": \"chirp-crow\"",
        ))
        .stdout(predicate::str::contains("\"serial_mutations\": false"));

    let mut show = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut show, &test_home)
        .args(["config", "show", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"default_model\": \"chirp-fenix\"",
        ))
        .stdout(predicate::str::contains("\"serial_mutations\": true"));
}

#[test]
fn config_check_json_reports_missing_auth_structurally() {
    let test_home = isolated_test_home("sunox-cli-config-check-test");

    let mut cmd = Command::cargo_bin("sunox").expect("binary");
    with_isolated_home(&mut cmd, &test_home)
        .args(["config", "check", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""))
        .stdout(predicate::str::contains("\"auth\""))
        .stdout(predicate::str::contains("\"ok\": false"))
        .stdout(predicate::str::contains("\"code\": \"auth_missing\""));
}

#[test]
fn agent_info_reports_submit_wait_download_workflow() {
    let mut cmd = Command::cargo_bin("sunox").expect("binary");

    cmd.args(["agent-info", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"clip wait\""))
        .stdout(predicate::str::contains("\"workflow\""))
        .stdout(predicate::str::contains("\"human_commands\""))
        .stdout(predicate::str::contains("\"machine_commands\""))
        .stdout(predicate::str::contains("\"execution_policy\""))
        .stdout(predicate::str::contains("\"agent_safety\""))
        .stdout(predicate::str::contains("\"post_submit_workflow\""))
        .stdout(predicate::str::contains("account-scoped"))
        .stdout(predicate::str::contains("do not pass --parallel"))
        .stdout(predicate::str::contains("\"audio_analysis\""))
        .stdout(predicate::str::contains("\"download_formats\""))
        .stdout(predicate::str::contains(
            "current CLI download supports MP3",
        ))
        .stdout(predicate::str::contains(
            "Suno Web exposes Pro download choices",
        ))
        .stdout(predicate::str::contains("WAV"))
        .stdout(predicate::str::contains("do not publish"))
        .stdout(predicate::str::contains("destructive commands require"))
        .stdout(predicate::str::contains("returned clip IDs"))
        .stdout(predicate::str::contains("--parallel"))
        .stdout(predicate::str::contains("partial_mutation"))
        .stdout(predicate::str::contains(
            "not the same as Suno Web Pro Get Stems export",
        ))
        .stdout(predicate::str::contains("sunox download <clip_id>"))
        .stdout(predicate::str::contains(
            "sunox add <clip_id> --to <playlist_id>",
        ))
        .stdout(predicate::str::contains(
            "submit generation or description and return clip payload",
        ))
        .stdout(predicate::str::contains(
            "poll clip ids until complete or error",
        ))
        .stdout(predicate::str::contains("download completed media"))
        .stdout(predicate::str::contains("\"v3.5\""))
        .stdout(predicate::str::contains("chirp-v3-5"))
        .stdout(predicate::str::contains("\"playlist\""))
        .stdout(predicate::str::contains("\"playlist_create\""))
        .stdout(predicate::str::contains("\"playlist_set_visibility\""))
        .stdout(predicate::str::contains("\"playlist_reorder_tracks\""))
        .stdout(predicate::str::contains("\"playlist_save\""))
        .stdout(predicate::str::contains("\"playlist_unsave\""))
        .stdout(predicate::str::contains("\"playlist_like\""))
        .stdout(predicate::str::contains("\"playlist_dislike\""))
        .stdout(predicate::str::contains("\"persona_create\""))
        .stdout(predicate::str::contains("\"persona_clips\""))
        .stdout(predicate::str::contains("\"persona_set_visibility\""))
        .stdout(predicate::str::contains("\"persona_set_metadata\""))
        .stdout(predicate::str::contains("\"persona_processed_clip\""))
        .stdout(predicate::str::contains("\"persona_love\""))
        .stdout(predicate::str::contains("\"persona_unlove\""))
        .stdout(predicate::str::contains("\"persona_toggle_love\""))
        .stdout(predicate::str::contains("\"clip_restore\""))
        .stdout(predicate::str::contains("\"clip_like\""))
        .stdout(predicate::str::contains("\"clip_dislike\""))
        .stdout(predicate::str::contains("\"clip_speed\""))
        .stdout(predicate::str::contains("\"persona_list\""))
        .stdout(predicate::str::contains("token=null"))
        .stdout(predicate::str::contains("--captcha"))
        .stdout(predicate::str::contains("\"audio_upload\""))
        .stdout(predicate::str::contains("\"persona_delete\""))
        .stdout(predicate::str::contains("\"persona_restore\""))
        .stdout(predicate::str::contains("\"persona_purge\""))
        .stdout(predicate::str::contains("\"unsupported_surfaces\""))
        .stdout(predicate::str::contains("\"image_upload\""))
        .stdout(predicate::str::contains("\"update_feedback_state\""))
        .stdout(predicate::str::contains("\"not_implemented\"").not())
        .stdout(predicate::str::contains("deprecated").not())
        .stdout(predicate::str::contains("\"config\""))
        .stdout(predicate::str::contains("\"serial_mutations\""))
        .stdout(predicate::str::contains("\"agent_targets\""))
        .stdout(predicate::str::contains("\"codex\""))
        .stdout(predicate::str::contains("~/.codex/skills/sunox/SKILL.md"))
        .stdout(predicate::str::contains(
            "sunox install-skill --target codex",
        ))
        .stdout(predicate::str::contains("clip speed"))
        .stdout(predicate::str::contains("config.toml"));
}

#[test]
fn agent_info_separates_challenge_capable_commands_from_async_edits() {
    let output = Command::cargo_bin("sunox")
        .expect("binary")
        .args(["agent-info", "--json"])
        .output()
        .expect("agent info output");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    let agent_info: serde_json::Value = serde_json::from_str(&stdout).expect("agent info json");
    let command_notes = agent_info["command_notes"]
        .as_object()
        .expect("command_notes object");

    let challenge_commands = command_notes["challenge_capable_generation_commands"]["commands"]
        .as_array()
        .expect("challenge commands");
    let challenge_commands = challenge_commands
        .iter()
        .map(|value| value.as_str().expect("command string"))
        .collect::<Vec<_>>();

    assert_eq!(
        challenge_commands,
        vec![
            "create",
            "describe",
            "clip cover",
            "clip extend",
            "clip stems"
        ]
    );
    for command in ["clip concat", "clip remaster", "clip speed"] {
        assert!(!challenge_commands.contains(&command));
    }

    let async_edits = command_notes["async_clip_edits"]["commands"]
        .as_array()
        .expect("async edit commands");
    let async_edits = async_edits
        .iter()
        .map(|value| value.as_str().expect("command string"))
        .collect::<Vec<_>>();

    for command in [
        "clip cover",
        "clip extend",
        "clip concat",
        "clip stems",
        "clip remaster",
        "clip speed",
    ] {
        assert!(async_edits.contains(&command));
    }
    let extend_notes = command_notes["clip extend"]
        .as_object()
        .expect("extend notes");
    assert_eq!(
        extend_notes["route"],
        "GET /api/feed/?ids=<clip_id>, optional POST /api/feed/v3 metadata fallback, then POST /api/generate/v2-web/"
    );
    assert!(
        extend_notes["defaults"]
            .as_str()
            .expect("extend defaults")
            .contains("source.metadata.make_instrumental")
    );
    assert!(
        extend_notes["defaults"]
            .as_str()
            .expect("extend defaults")
            .contains("source.metadata.negative_tags")
    );
    assert!(command_notes.get("generate_backed_clip_edits").is_none());
}
