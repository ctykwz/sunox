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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_model: String,
    pub poll_interval_secs: u64,
    pub poll_timeout_secs: u64,
    pub output_dir: String,
    pub serial_mutations: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            default_model: "auto".into(),
            poll_interval_secs: 5,
            poll_timeout_secs: 600,
            output_dir: ".".into(),
            serial_mutations: true,
        }
    }
}

const VALID_CONFIG_KEYS: &str =
    "default_model, poll_interval_secs, poll_timeout_secs, output_dir, serial_mutations";

impl AppConfig {
    pub fn load() -> Result<Self, CliError> {
        Self::load_from_path(Self::path(), std::env::vars())
    }

    pub fn load_with_overrides(overrides: &[String]) -> Result<Self, CliError> {
        let mut config = Self::load()?;
        config.apply_overrides(overrides)?;
        Ok(config)
    }

    pub(crate) fn load_from_path<I>(
        path: Option<std::path::PathBuf>,
        vars: I,
    ) -> Result<Self, CliError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut figment = Figment::new().merge(Serialized::defaults(AppConfig::default()));
        if let Some(path) = path {
            figment = figment.merge(Toml::file(path));
        }
        let mut config: AppConfig = figment
            .extract()
            .map_err(|e| CliError::Config(format!("parse config: {e}")))?;
        config.apply_env_overrides(vars)?;
        ensure_poll_interval_secs(config.poll_interval_secs)?;
        ensure_poll_timeout_secs(config.poll_timeout_secs)?;
        Ok(config)
    }

    pub fn path() -> Option<std::path::PathBuf> {
        directories::ProjectDirs::from("com", "sunox", "sunox")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    pub fn set_persisted(key: &str, value: &str) -> Result<Self, CliError> {
        let path =
            Self::path().ok_or_else(|| CliError::Config("could not resolve config path".into()))?;
        let lock_path = path.with_extension("lock");
        update_persisted_config(&path, &lock_path, key, value)?;
        Self::load()
    }

    fn apply_env_overrides<I>(&mut self, vars: I) -> Result<(), CliError>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        for (key, value) in vars {
            match key.as_str() {
                "SUNOX_DEFAULT_MODEL" => self.default_model = normalize_model_key(&value)?,
                "SUNOX_POLL_INTERVAL_SECS" => {
                    self.poll_interval_secs =
                        parse_poll_interval("SUNOX_POLL_INTERVAL_SECS", &value)?;
                }
                "SUNOX_POLL_TIMEOUT_SECS" => {
                    self.poll_timeout_secs = parse_poll_timeout("SUNOX_POLL_TIMEOUT_SECS", &value)?;
                }
                "SUNOX_OUTPUT_DIR" => self.output_dir = value,
                "SUNOX_SERIAL_MUTATIONS" => {
                    self.serial_mutations = parse_bool("SUNOX_SERIAL_MUTATIONS", &value)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn apply_overrides(&mut self, overrides: &[String]) -> Result<(), CliError> {
        for override_value in overrides {
            let (key, value) = override_value.split_once('=').ok_or_else(|| {
                CliError::Config(format!(
                    "config override `{override_value}` must use key=value syntax"
                ))
            })?;
            self.set_value(key.trim(), normalize_override_value(value.trim()))?;
        }
        Ok(())
    }

    fn set_value(&mut self, key: &str, value: String) -> Result<(), CliError> {
        match key {
            "default_model" => self.default_model = normalize_model_key(&value)?,
            "poll_interval_secs" => self.poll_interval_secs = parse_poll_interval(key, &value)?,
            "poll_timeout_secs" => self.poll_timeout_secs = parse_poll_timeout(key, &value)?,
            "output_dir" => self.output_dir = value,
            "serial_mutations" => self.serial_mutations = parse_bool(key, &value)?,
            _ => {
                return Err(CliError::Config(format!(
                    "unknown config key `{key}`; valid keys: {VALID_CONFIG_KEYS}"
                )));
            }
        }
        Ok(())
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
    let mut stored = StoredConfig::load(path)?;
    stored.set(key, value)?;
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
}

impl StoredConfig {
    fn load(path: &std::path::Path) -> Result<Self, CliError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let data = std::fs::read_to_string(path)?;
        toml::from_str(&data).map_err(|e| CliError::Config(format!("parse config: {e}")))
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), CliError> {
        match key {
            "default_model" => self.default_model = Some(normalize_model_key(value)?),
            "poll_interval_secs" => {
                self.poll_interval_secs = Some(parse_poll_interval(key, value)?)
            }
            "poll_timeout_secs" => self.poll_timeout_secs = Some(parse_poll_timeout(key, value)?),
            "output_dir" => self.output_dir = Some(value.to_string()),
            "serial_mutations" => self.serial_mutations = Some(parse_bool(key, value)?),
            _ => {
                return Err(CliError::Config(format!(
                    "unknown config key `{key}`; valid keys: {VALID_CONFIG_KEYS}"
                )));
            }
        }
        Ok(())
    }
}

fn normalize_model_key(value: &str) -> Result<String, CliError> {
    let normalized = match value {
        "auto" => "auto",
        "v5.5" | "chirp-fenix" => "chirp-fenix",
        "v5" | "chirp-crow" => "chirp-crow",
        "v4.5+" | "chirp-bluejay" => "chirp-bluejay",
        "v4.5-all" | "chirp-auk-turbo" => "chirp-auk-turbo",
        "v4.5" | "chirp-auk" => "chirp-auk",
        "v4" | "chirp-v4" => "chirp-v4",
        "v3.5" | "chirp-v3-5" => "chirp-v3-5",
        "v3" | "chirp-v3-0" => "chirp-v3-0",
        "v2" | "chirp-v2-xxl-alpha" => "chirp-v2-xxl-alpha",
        _ => {
            return Err(CliError::Config(format!(
                "unknown model `{value}`; use auto, a CLI model version such as v5.5, or a Suno API model key such as chirp-fenix"
            )));
        }
    };
    Ok(normalized.to_string())
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

    use super::{AppConfig, StoredConfig, update_persisted_config};

    #[test]
    fn stored_config_sets_known_string_key() {
        let mut config = StoredConfig::default();

        config.set("default_model", "v5.5").expect("set config");

        assert_eq!(config.default_model.as_deref(), Some("chirp-fenix"));
    }

    #[test]
    fn stored_config_accepts_the_current_free_model() {
        let mut config = StoredConfig::default();

        config
            .set("default_model", "v4.5-all")
            .expect("set free model");

        assert_eq!(config.default_model.as_deref(), Some("chirp-auk-turbo"));
    }

    #[test]
    fn stored_config_rejects_unknown_default_model() {
        let mut config = StoredConfig::default();

        let err = config
            .set("default_model", "unknown-model")
            .expect_err("unknown model");

        assert!(err.to_string().contains("unknown model"));
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
    fn generation_model_defaults_to_account_auto_selection() {
        let config = AppConfig::default();

        assert_eq!(config.default_model, "auto");
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
        let mut config = AppConfig::default();

        config
            .apply_env_overrides([
                ("SUNOX_DEFAULT_MODEL".to_string(), "v5".to_string()),
                ("SUNOX_POLL_INTERVAL_SECS".to_string(), "9".to_string()),
                ("SUNOX_POLL_TIMEOUT_SECS".to_string(), "777".to_string()),
                (
                    "SUNOX_OUTPUT_DIR".to_string(),
                    "/tmp/suno-output".to_string(),
                ),
                ("SUNOX_SERIAL_MUTATIONS".to_string(), "false".to_string()),
            ])
            .expect("env overrides");

        assert_eq!(config.default_model, "chirp-crow");
        assert_eq!(config.poll_interval_secs, 9);
        assert_eq!(config.poll_timeout_secs, 777);
        assert_eq!(config.output_dir, "/tmp/suno-output");
        assert!(!config.serial_mutations);
    }

    #[test]
    fn serial_mutations_override_accepts_boolean_value() {
        let mut config = AppConfig::default();

        config
            .apply_overrides(&["serial_mutations=false".to_string()])
            .expect("apply override");

        assert!(!config.serial_mutations);
    }

    #[test]
    fn serial_mutations_rejects_non_boolean_value() {
        let mut config = AppConfig::default();

        let err = config
            .apply_overrides(&["serial_mutations=fast".to_string()])
            .expect_err("invalid bool");

        assert!(err.to_string().contains("expects true or false"));
    }

    #[test]
    fn env_override_rejects_unknown_default_model() {
        let mut config = AppConfig::default();

        let err = config
            .apply_env_overrides([(
                "SUNOX_DEFAULT_MODEL".to_string(),
                "unknown-model".to_string(),
            )])
            .expect_err("unknown model");

        assert!(err.to_string().contains("unknown model"));
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
