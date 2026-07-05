use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::core::CliError;

use super::refresh_lock::AuthRefreshLockGuard;
use super::types::BrowserEnvironment;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct AuthState {
    pub jwt: Option<String>,
    pub cookie: Option<String>,
    pub session_id: Option<String>,
    pub device_id: Option<String>,
    pub browser_environment: Option<BrowserEnvironment>,
    /// The __client cookie from clerk domain - long-lived (~7 days)
    pub clerk_client_cookie: Option<String>,
}

impl AuthState {
    pub fn load() -> Result<Self, CliError> {
        let path = Self::path();
        if !path.exists() {
            return Err(CliError::AuthMissing);
        }
        let data = std::fs::read_to_string(&path)?;
        serde_json::from_str(&data).map_err(|e| CliError::Config(format!("corrupt auth file: {e}")))
    }

    pub fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        self.save_to_path_with_account_lock(&path)
    }

    pub(crate) fn save_without_account_lock(&self) -> Result<(), CliError> {
        let path = Self::path();
        self.save_to_path(&path)
    }

    fn save_to_path_with_account_lock(&self, path: &Path) -> Result<(), CliError> {
        let _guard = AuthRefreshLockGuard::acquire(self)?;
        self.save_to_path(path)
    }

    #[cfg(test)]
    fn save_to_path_with_lock_path(&self, path: &Path, lock_path: &Path) -> Result<(), CliError> {
        let _guard = AuthRefreshLockGuard::acquire_path(lock_path)?;
        self.save_to_path(path)
    }

    fn save_to_path(&self, path: &Path) -> Result<(), CliError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let data = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .mode(0o600)
                .open(&tmp)?;
            file.write_all(data.as_bytes())?;
            file.sync_all()?;
        }

        #[cfg(not(unix))]
        {
            std::fs::write(&tmp, &data)?;
        }

        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn delete() -> Result<(), CliError> {
        let path = Self::path();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    pub fn is_jwt_expired(&self) -> bool {
        let Some(jwt) = &self.jwt else { return true };
        let parts: Vec<&str> = jwt.split('.').collect();
        if parts.len() != 3 {
            return true;
        }
        let claims = parts[1];
        let Ok(decoded) = BASE64URL.decode(claims) else {
            return true;
        };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&decoded) else {
            return true;
        };
        let Some(exp) = value.get("exp").and_then(|v| v.as_u64()) else {
            return true;
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Suno issues 1-hour JWTs, but generation can reject older tokens
        // before `exp`; refresh any JWT with under 30 minutes left.
        now + 1800 >= exp
    }

    pub fn account_lock_key(&self) -> Result<String, CliError> {
        let source = self
            .jwt_account_subject()
            .map(|subject| format!("jwt-sub:{subject}"))
            .or_else(|| {
                self.session_id
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("session:{value}"))
            })
            .or_else(|| {
                self.clerk_client_cookie
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("clerk-client:{value}"))
            })
            .or_else(|| {
                self.cookie
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(|value| format!("cookie:{value}"))
            })
            .ok_or(CliError::AuthMissing)?;

        Ok(format!("account-{}", sha256_hex(source.as_bytes())))
    }

    pub(crate) fn matches_account_material(&self, other: &Self) -> bool {
        let self_subject = self.jwt_account_subject();
        let other_subject = other.jwt_account_subject();
        if let (Some(self_subject), Some(other_subject)) = (self_subject, other_subject) {
            return self_subject == other_subject;
        }

        same_non_empty(self.session_id.as_deref(), other.session_id.as_deref())
            || same_non_empty(
                self.clerk_client_cookie.as_deref(),
                other.clerk_client_cookie.as_deref(),
            )
            || same_non_empty(self.cookie.as_deref(), other.cookie.as_deref())
    }

    fn jwt_account_subject(&self) -> Option<String> {
        let jwt = self.jwt.as_deref()?;
        let claims = decode_jwt_claims(jwt)?;
        ["sub", "user_id", "id"].into_iter().find_map(|field| {
            claims
                .get(field)
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
    }

    fn path() -> PathBuf {
        directories::ProjectDirs::from("com", "sunox", "sunox")
            .map(|dirs| dirs.config_dir().join("auth.json"))
            .unwrap_or_else(|| PathBuf::from("~/.config/sunox/auth.json"))
    }
}

fn decode_jwt_claims(jwt: &str) -> Option<serde_json::Value> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let decoded = BASE64URL.decode(parts[1]).ok()?;
    serde_json::from_slice::<serde_json::Value>(&decoded).ok()
}

fn sha256_hex(input: &[u8]) -> String {
    Sha256::digest(input)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn same_non_empty(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => !left.is_empty() && left == right,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};
    use std::thread;

    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD as BASE64URL;

    use super::AuthState;

    fn jwt_with_subject(subject: &str) -> String {
        let header = BASE64URL.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let claims = BASE64URL.encode(format!(r#"{{"sub":"{subject}","exp":4102444800}}"#));
        format!("{header}.{claims}.signature")
    }

    #[test]
    fn concurrent_saves_to_same_auth_path_do_not_share_temp_file() {
        let dir =
            std::env::temp_dir().join(format!("sunox-auth-state-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("test dir");
        let path = dir.join("auth.json");
        let barrier = Arc::new(Barrier::new(8));

        let handles = (0..8)
            .map(|index| {
                let path = path.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let auth = AuthState {
                        jwt: Some(format!("jwt-{index}")),
                        clerk_client_cookie: Some(format!("client-{index}")),
                        ..Default::default()
                    };
                    barrier.wait();
                    auth.save_to_path(&path)
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().expect("save thread").expect("save result");
        }

        let saved = std::fs::read_to_string(&path).expect("saved auth file");
        serde_json::from_str::<AuthState>(&saved).expect("valid saved auth json");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn locked_save_creates_lock_file_and_writes_auth_state() {
        let dir =
            std::env::temp_dir().join(format!("sunox-auth-locked-save-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("test dir");
        let path = dir.join("auth.json");
        let lock_path = dir.join("auth.lock");
        let auth = AuthState {
            jwt: Some(jwt_with_subject("locked-save-user")),
            clerk_client_cookie: Some("client".into()),
            ..Default::default()
        };

        auth.save_to_path_with_lock_path(&path, &lock_path)
            .expect("locked save");

        assert!(lock_path.exists());
        let saved = std::fs::read_to_string(&path).expect("saved auth file");
        let saved = serde_json::from_str::<AuthState>(&saved).expect("valid saved auth json");
        assert_eq!(saved.jwt, auth.jwt);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn account_lock_key_prefers_jwt_subject() {
        let first = AuthState {
            jwt: Some(jwt_with_subject("user_same")),
            session_id: Some("session-a".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };
        let second = AuthState {
            jwt: Some(jwt_with_subject("user_same")),
            session_id: Some("session-b".into()),
            clerk_client_cookie: Some("cookie-b".into()),
            ..Default::default()
        };
        let other = AuthState {
            jwt: Some(jwt_with_subject("user_other")),
            session_id: Some("session-a".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };

        assert_eq!(
            first.account_lock_key().expect("first lock key"),
            second.account_lock_key().expect("second lock key")
        );
        assert_ne!(
            first.account_lock_key().expect("first lock key"),
            other.account_lock_key().expect("other lock key")
        );
    }

    #[test]
    fn account_lock_key_falls_back_to_session_id() {
        let first = AuthState {
            session_id: Some("session-same".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };
        let second = AuthState {
            session_id: Some("session-same".into()),
            clerk_client_cookie: Some("cookie-b".into()),
            ..Default::default()
        };
        let other = AuthState {
            session_id: Some("session-other".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };

        assert_eq!(
            first.account_lock_key().expect("first lock key"),
            second.account_lock_key().expect("second lock key")
        );
        assert_ne!(
            first.account_lock_key().expect("first lock key"),
            other.account_lock_key().expect("other lock key")
        );
    }

    #[test]
    fn account_lock_key_hashes_cookie_material() {
        let raw_cookie = "raw-secret-client-cookie-value";
        let auth = AuthState {
            clerk_client_cookie: Some(raw_cookie.into()),
            ..Default::default()
        };

        let key = auth.account_lock_key().expect("lock key");

        assert!(!key.contains("raw-secret-client-cookie-value"));
    }

    #[test]
    fn account_lock_key_can_use_full_cookie_header() {
        let auth = AuthState {
            cookie: Some("session=raw-session-cookie; __client=raw-client-cookie".into()),
            ..Default::default()
        };

        let key = auth.account_lock_key().expect("lock key");

        assert!(!key.contains("__client"));
        assert!(!key.contains("raw-session-cookie"));
        assert!(!key.contains("raw-client-cookie"));
    }

    #[test]
    fn account_material_requires_matching_jwt_subjects_when_both_exist() {
        let first = AuthState {
            jwt: Some(jwt_with_subject("user-a")),
            session_id: Some("session-same".into()),
            clerk_client_cookie: Some("cookie-same".into()),
            ..Default::default()
        };
        let second = AuthState {
            jwt: Some(jwt_with_subject("user-b")),
            session_id: Some("session-same".into()),
            clerk_client_cookie: Some("cookie-same".into()),
            ..Default::default()
        };

        assert!(!first.matches_account_material(&second));
    }

    #[test]
    fn account_material_falls_back_to_session_when_subject_is_missing() {
        let first = AuthState {
            session_id: Some("session-same".into()),
            clerk_client_cookie: Some("cookie-a".into()),
            ..Default::default()
        };
        let second = AuthState {
            jwt: Some(jwt_with_subject("user-a")),
            session_id: Some("session-same".into()),
            clerk_client_cookie: Some("cookie-b".into()),
            ..Default::default()
        };

        assert!(first.matches_account_material(&second));
    }
}
