use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::CliError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChallengeBrowserMode {
    #[default]
    Auto,
    Existing,
    Isolated,
}

impl ChallengeBrowserMode {
    fn parse(key: &str, value: &str) -> Result<Self, CliError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "existing" => Ok(Self::Existing),
            "isolated" => Ok(Self::Isolated),
            _ => Err(CliError::Config(format!(
                "config key `{key}` expects auto, existing, or isolated"
            ))),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_model: String,
    pub poll_interval_secs: u64,
    pub poll_timeout_secs: u64,
    pub output_dir: String,
    pub serial_mutations: bool,
    pub challenge_browser: ChallengeBrowserMode,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_model: "chirp-hawk".into(),
            poll_interval_secs: 5,
            poll_timeout_secs: 600,
            output_dir: ".".into(),
            serial_mutations: true,
            challenge_browser: ChallengeBrowserMode::Auto,
        }
    }
}

const VALID_CONFIG_KEYS: &str = "default_model, poll_interval_secs, poll_timeout_secs, output_dir, serial_mutations, challenge_browser";

impl AppConfig {
    pub fn load() -> Result<Self, CliError> {
        Self::load_from_path(Self::path(), std::env::vars())
    }

    pub fn load_with_overrides(overrides: &[String]) -> Result<Self, CliError> {
        if overrides.is_empty() {
            return Self::load();
        }
        Self::load_from_path_with_overrides(Self::path(), std::env::vars(), overrides)
    }

    pub(crate) fn load_from_path<I>(
        path: Option<std::path::PathBuf>,
        vars: I,
    ) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        Self::load_from_path_with_overrides(path, vars, &[])
    }

    fn load_from_path_with_overrides<I>(
        path: Option<std::path::PathBuf>,
        vars: I,
        overrides: &[String],
    ) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        // Resolve precedence before parsing any winning value. A valid CLI
        // override must be able to replace an invalid environment or TOML
        // value, including a persisted value with the wrong TOML type.
        let mut winners = BTreeMap::new();
        for (key, value) in vars {
            if let Some(key) = environment_config_key(&key) {
                winners.insert(key.to_string(), value);
            }
        }
        for override_value in overrides {
            let (key, value) = override_value.split_once('=').ok_or_else(|| {
                CliError::Config(format!(
                    "config override `{override_value}` must use key=value syntax"
                ))
            })?;
            winners.insert(
                key.trim().to_string(),
                normalize_override_value(value.trim()),
            );
        }
        let mut overlay = StoredConfig::default();
        for (key, value) in winners {
            overlay.set(&key, &value)?;
        }
        let mut figment = Figment::new().merge(Serialized::defaults(AppConfig::default()));
        if let Some(path) = path {
            figment = figment.merge(Toml::file(path));
        }
        let mut config: AppConfig = figment
            .merge(Serialized::defaults(overlay))
            .extract()
            .map_err(|e| CliError::Config(format!("parse config: {e}")))?;
        // Canonicalize the merged configuration through the same gate used
        // by config writes. Model
        // availability is account-specific and is validated against billing
        // immediately before a generation submission.
        config.default_model = normalize_generation_model_selector(&config.default_model)?;
        ensure_poll_interval_secs(config.poll_interval_secs)?;
        ensure_poll_timeout_secs(config.poll_timeout_secs)?;
        Ok(config)
    }

    pub fn path() -> Option<std::path::PathBuf> {
        super::project_config_dir().map(|dir| dir.join("config.toml"))
    }

    pub fn set_persisted(key: &str, value: &str) -> Result<(), CliError> {
        let path =
            Self::path().ok_or_else(|| CliError::Config("could not resolve config path".into()))?;
        let lock_path = path.with_extension("lock");
        update_persisted_config(&path, &lock_path, key, value)
    }
}

fn environment_config_key(key: &str) -> Option<&'static str> {
    match key {
        "SUNOX_DEFAULT_MODEL" => Some("default_model"),
        "SUNOX_POLL_INTERVAL_SECS" => Some("poll_interval_secs"),
        "SUNOX_POLL_TIMEOUT_SECS" => Some("poll_timeout_secs"),
        "SUNOX_OUTPUT_DIR" => Some("output_dir"),
        "SUNOX_SERIAL_MUTATIONS" => Some("serial_mutations"),
        "SUNOX_CHALLENGE_BROWSER" => Some("challenge_browser"),
        _ => None,
    }
}

struct ConfigLockGuard {
    file: File,
}

impl ConfigLockGuard {
    fn acquire(path: &Path) -> Result<Self, CliError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }
}

impl Drop for ConfigLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn update_persisted_config(
    path: &Path,
    lock_path: &Path,
    key: &str,
    value: &str,
) -> Result<(), CliError> {
    let _guard = ConfigLockGuard::acquire(lock_path)?;
    let mut stored: toml::Table = if path.exists() {
        toml::from_str(&std::fs::read_to_string(path)?)
            .map_err(|error| CliError::Config(format!("parse config: {error}")))?
    } else {
        toml::Table::new()
    };
    // Validate only the field being repaired. Preserve unrelated invalid
    // fields and unknown keys so a targeted repair never resets user data.
    let mut replacement = StoredConfig::default();
    replacement.set(key, value)?;
    let replacement = toml::Value::try_from(replacement)
        .map_err(|error| CliError::Config(format!("serialize config: {error}")))?;
    let value = replacement
        .get(key)
        .expect("validated config key is serialized")
        .clone();
    stored.insert(key.to_string(), value);
    let data = toml::to_string_pretty(&stored)
        .map_err(|error| CliError::Config(format!("serialize config: {error}")))?;
    atomic_write(path, data.as_bytes())
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), CliError> {
    let parent = path
        .parent()
        .ok_or_else(|| CliError::Config("config path has no parent directory".into()))?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(data)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| CliError::Io(error.error))?;

    #[cfg(unix)]
    File::open(parent)?.sync_all()?;

    Ok(())
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct StoredConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    default_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    poll_interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    poll_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    serial_mutations: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge_browser: Option<ChallengeBrowserMode>,
}

impl StoredConfig {
    #[cfg(test)]
    fn load(path: &std::path::Path) -> Result<Self, CliError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(path)?;
        toml::from_str(&data).map_err(|e| CliError::Config(format!("parse config: {e}")))
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), CliError> {
        match key {
            "default_model" => {
                self.default_model = Some(normalize_generation_model_selector(value)?)
            }
            "poll_interval_secs" => {
                self.poll_interval_secs = Some(parse_poll_interval(key, value)?)
            }
            "poll_timeout_secs" => self.poll_timeout_secs = Some(parse_poll_timeout(key, value)?),
            "output_dir" => self.output_dir = Some(value.to_string()),
            "serial_mutations" => self.serial_mutations = Some(parse_bool(key, value)?),
            "challenge_browser" => {
                self.challenge_browser = Some(ChallengeBrowserMode::parse(key, value)?);
            }
            _ => {
                return Err(CliError::Config(format!(
                    "unknown config key `{key}`; valid keys: {VALID_CONFIG_KEYS}"
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn normalize_generation_model_selector(value: &str) -> Result<String, CliError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(CliError::Config(
            "generation model selector cannot be empty".into(),
        ));
    }
    let lower = value.to_ascii_lowercase();
    match lower.as_str() {
        "auto" | "v6" | "v6-wild" | "v6-mini" | "v5.5" | "v5" | "v4.5+" | "v4.5-all" | "v4.5"
        | "v4" | "v3.5" | "v3" | "v2" | "chirp-hawk" | "chirp-hawk-wild" | "chirp-goose"
        | "chirp-fenix" | "chirp-crow" | "chirp-bluejay" | "chirp-auk-turbo" | "chirp-auk"
        | "chirp-v4" | "chirp-v3-5" | "chirp-v3-0" | "chirp-v2-xxl-alpha" => Ok(lower),
        _ => Ok(value.to_string()),
    }
}

fn normalize_override_value(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
        .to_string()
}

fn parse_u64(key: &str, value: &str) -> Result<u64, CliError> {
    value
        .parse::<u64>()
        .map_err(|_| CliError::Config(format!("config key `{key}` expects an unsigned integer")))
}

fn parse_poll_timeout(key: &str, value: &str) -> Result<u64, CliError> {
    let value = parse_u64(key, value)?;
    ensure_poll_timeout_secs(value)?;
    Ok(value)
}

fn parse_poll_interval(key: &str, value: &str) -> Result<u64, CliError> {
    let value = parse_u64(key, value)?;
    ensure_poll_interval_secs(value)?;
    Ok(value)
}

fn ensure_poll_interval_secs(value: u64) -> Result<(), CliError> {
    super::polling::ensure_poll_interval(std::time::Duration::from_secs(value))
}

pub fn ensure_poll_timeout_secs(value: u64) -> Result<(), CliError> {
    super::polling::ensure_poll_timeout(std::time::Duration::from_secs(value))
}

fn parse_bool(key: &str, value: &str) -> Result<bool, CliError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CliError::Config(format!(
            "config key `{key}` expects true or false"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use crate::core::CliError;

    use super::{AppConfig, ChallengeBrowserMode, StoredConfig, update_persisted_config};

    #[test]
    fn stored_config_sets_known_string_key() {
        let mut config = StoredConfig::default();

        config.set("default_model", "v5.5").expect("set config");

        assert_eq!(config.default_model.as_deref(), Some("v5.5"));
    }

    #[test]
    fn stored_config_accepts_the_current_free_model() {
        let mut config = StoredConfig::default();

        config
            .set("default_model", "v4.5-all")
            .expect("set free model");

        assert_eq!(config.default_model.as_deref(), Some("v4.5-all"));
    }

    #[test]
    fn stored_config_preserves_v2_for_account_validation() {
        let mut config = StoredConfig::default();

        config
            .set("default_model", "v2")
            .expect("account billing decides whether v2 remains usable");

        assert_eq!(config.default_model.as_deref(), Some("v2"));
    }

    #[test]
    fn load_does_not_migrate_a_live_v2_selector_to_auto() {
        let dir = tempfile::tempdir().expect("test dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_model = \"chirp-v2-xxl-alpha\"\n")
            .expect("write legacy config");

        let config = AppConfig::load_from_path(Some(path), []).expect("load persisted model");

        assert_eq!(config.default_model, "chirp-v2-xxl-alpha");
    }

    #[test]
    fn load_preserves_a_supported_display_selector_from_an_existing_config_file() {
        let dir = tempfile::tempdir().expect("test dir");
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "default_model = \"v5.5\"\n").expect("write config");

        let config = AppConfig::load_from_path(Some(path), []).expect("load config");

        assert_eq!(config.default_model, "v5.5");
    }

    #[test]
    fn stored_config_canonicalizes_an_exact_external_model_key() {
        let mut config = StoredConfig::default();

        config
            .set("default_model", "CHIRP-FENIX")
            .expect("known external key");

        assert_eq!(config.default_model.as_deref(), Some("chirp-fenix"));
    }

    #[test]
    fn stored_config_accepts_account_model_selector() {
        let mut config = StoredConfig::default();

        config
            .set("default_model", "  account-model-id  ")
            .expect("billing validates account model selectors before submit");

        assert_eq!(config.default_model.as_deref(), Some("account-model-id"));
    }

    #[test]
    fn stored_config_parses_numeric_keys() {
        let mut config = StoredConfig::default();

        config.set("poll_timeout_secs", "900").expect("set config");

        assert_eq!(config.poll_timeout_secs, Some(900));
    }

    #[test]
    fn serial_mutations_defaults_to_true() {
        let config = AppConfig::default();

        assert!(config.serial_mutations);
    }

    #[test]
    fn challenge_browser_defaults_to_auto() {
        let config = AppConfig::default();

        assert_eq!(config.challenge_browser, ChallengeBrowserMode::Auto);
    }

    #[test]
    fn challenge_browser_accepts_all_supported_modes() {
        let mut config = StoredConfig::default();

        for (value, expected) in [
            ("auto", ChallengeBrowserMode::Auto),
            ("existing", ChallengeBrowserMode::Existing),
            ("isolated", ChallengeBrowserMode::Isolated),
        ] {
            config
                .set("challenge_browser", value)
                .expect("set challenge browser");
            assert_eq!(config.challenge_browser, Some(expected));
        }
    }

    #[test]
    fn challenge_browser_rejects_unknown_mode() {
        let mut config = StoredConfig::default();

        let error = config
            .set("challenge_browser", "headless")
            .expect_err("reject unsupported mode");

        assert!(error.to_string().contains("auto, existing, or isolated"));
    }

    #[test]
    fn generation_model_defaults_to_v6_pro() {
        let config = AppConfig::default();

        assert_eq!(config.default_model, "chirp-hawk");
    }

    #[test]
    fn serial_mutations_can_be_set_persistently() {
        let mut config = StoredConfig::default();

        config.set("serial_mutations", "false").expect("set config");

        assert_eq!(config.serial_mutations, Some(false));
    }

    #[test]
    fn stored_config_rejects_unknown_keys() {
        let mut config = StoredConfig::default();

        let err = config.set("missing", "value").expect_err("unknown key");

        assert!(err.to_string().contains("unknown config key"));
    }

    #[test]
    fn concurrent_persisted_updates_preserve_both_fields() {
        let dir = tempfile::tempdir().expect("test dir");
        let path = dir.path().join("config.toml");
        let lock_path = dir.path().join("config.lock");
        let barrier = Arc::new(Barrier::new(3));
        let handles = [
            ("poll_timeout_secs", "777"),
            ("output_dir", "/tmp/sunox-concurrent"),
        ]
        .into_iter()
        .map(|(key, value)| {
            let path = path.clone();
            let lock_path = lock_path.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                update_persisted_config(&path, &lock_path, key, value).expect("persist config");
            })
        })
        .collect::<Vec<_>>();

        barrier.wait();
        for handle in handles {
            handle.join().expect("config writer");
        }

        let stored = StoredConfig::load(&path).expect("stored config");
        assert_eq!(stored.poll_timeout_secs, Some(777));
        assert_eq!(stored.output_dir.as_deref(), Some("/tmp/sunox-concurrent"));
        assert!(
            dir.path()
                .read_dir()
                .expect("config directory")
                .all(|entry| !entry
                    .expect("directory entry")
                    .file_name()
                    .to_string_lossy()
                    .contains("tmp"))
        );
    }

    #[test]
    fn env_overrides_support_underscored_config_keys() {
        let config = AppConfig::load_from_path(
            None,
            [
                ("SUNOX_DEFAULT_MODEL".to_string(), "v5".to_string()),
                ("SUNOX_POLL_INTERVAL_SECS".to_string(), "9".to_string()),
                ("SUNOX_POLL_TIMEOUT_SECS".to_string(), "777".to_string()),
                (
                    "SUNOX_OUTPUT_DIR".to_string(),
                    "/tmp/suno-output".to_string(),
                ),
                ("SUNOX_SERIAL_MUTATIONS".to_string(), "false".to_string()),
            ],
        )
        .expect("env overrides");

        assert_eq!(config.default_model, "v5");
        assert_eq!(config.poll_interval_secs, 9);
        assert_eq!(config.poll_timeout_secs, 777);
        assert_eq!(config.output_dir, "/tmp/suno-output");
        assert!(!config.serial_mutations);
    }

    #[test]
    fn serial_mutations_override_accepts_boolean_value() {
        let config = AppConfig::load_from_path_with_overrides(
            None,
            [],
            &["serial_mutations=false".to_string()],
        )
        .expect("apply override");

        assert!(!config.serial_mutations);
    }

    #[test]
    fn serial_mutations_rejects_non_boolean_value() {
        let err = AppConfig::load_from_path_with_overrides(
            None,
            [],
            &["serial_mutations=fast".to_string()],
        )
        .expect_err("invalid bool");

        assert!(err.to_string().contains("expects true or false"));
    }

    #[test]
    fn env_override_accepts_account_display_name() {
        let config = AppConfig::load_from_path(
            None,
            [(
                "SUNOX_DEFAULT_MODEL".to_string(),
                "My Account Model".to_string(),
            )],
        )
        .expect("account display name");

        assert_eq!(config.default_model, "My Account Model");
    }

    #[test]
    fn empty_generation_model_selector_is_rejected() {
        let mut config = StoredConfig::default();

        let error = config
            .set("default_model", "  ")
            .expect_err("empty selector");

        assert!(error.to_string().contains("cannot be empty"));
    }

    #[test]
    fn load_from_path_reports_invalid_toml() {
        let path = std::env::temp_dir().join(format!(
            "sunox-invalid-config-{}-{}.toml",
            std::process::id(),
            "core"
        ));
        std::fs::write(&path, "poll_timeout_secs = \"slow\"").expect("write config");

        let err = AppConfig::load_from_path(Some(path.clone()), []).expect_err("invalid config");

        let _ = std::fs::remove_file(path);
        assert!(err.to_string().contains("parse config"));
    }

    #[test]
    fn env_override_rejects_zero_poll_timeout() {
        let error =
            AppConfig::load_from_path(None, [("SUNOX_POLL_TIMEOUT_SECS".into(), "0".into())])
                .expect_err("zero poll timeout must be rejected");

        assert!(matches!(error, CliError::Config(message) if message.contains("greater than 0")));
    }

    #[test]
    fn env_override_rejects_zero_poll_interval() {
        let error =
            AppConfig::load_from_path(None, [("SUNOX_POLL_INTERVAL_SECS".into(), "0".into())])
                .expect_err("zero poll interval must be rejected");

        assert!(
            matches!(error, CliError::Config(message) if message.contains("poll interval") && message.contains("greater than 0"))
        );
    }

    #[test]
    fn env_override_rejects_poll_timeout_that_overflows_instant() {
        let error = AppConfig::load_from_path(
            None,
            [("SUNOX_POLL_TIMEOUT_SECS".into(), u64::MAX.to_string())],
        )
        .expect_err("overflowing poll timeout must be rejected");

        assert!(matches!(error, CliError::Config(message) if message.contains("too large")));
    }
}
