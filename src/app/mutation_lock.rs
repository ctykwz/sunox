use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use fs2::FileExt;

use crate::auth::AuthState;
use crate::core::CliError;

pub struct MutationLockGuard {
    file: File,
}

impl MutationLockGuard {
    pub fn acquire(auth: &AuthState) -> Result<Self, CliError> {
        let key = auth.account_lock_key()?;
        let path = lock_file_path(&key)?;
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

impl Drop for MutationLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn lock_file_path(key: &str) -> Result<PathBuf, CliError> {
    let dir = crate::core::project_config_dir()
        .map(|dir| dir.join("locks"))
        .ok_or_else(|| CliError::Config("cannot resolve sunox config directory".into()))?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("mutation-{key}.lock")))
}
