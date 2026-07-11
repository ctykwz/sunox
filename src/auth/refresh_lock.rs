use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use super::AuthState;
use crate::core::CliError;

pub(crate) struct AuthRefreshLockGuard {
    file: File,
}

pub(crate) struct AuthStateLockGuard {
    file: File,
}

impl AuthRefreshLockGuard {
    pub(crate) fn acquire(auth: &AuthState) -> Result<Self, CliError> {
        let path = lock_file_path(auth)?;
        Self::acquire_path(&path)
    }

    pub(crate) fn acquire_path(path: &Path) -> Result<Self, CliError> {
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

impl Drop for AuthRefreshLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

impl AuthStateLockGuard {
    pub(crate) fn acquire() -> Result<Self, CliError> {
        let dir = crate::core::project_config_dir()
            .map(|dir| dir.join("locks"))
            .ok_or_else(|| CliError::Config("cannot resolve sunox config directory".into()))?;
        Self::acquire_path(&dir.join("auth-state.lock"))
    }

    pub(crate) fn acquire_path(path: &Path) -> Result<Self, CliError> {
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

impl Drop for AuthStateLockGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn lock_file_path(auth: &AuthState) -> Result<PathBuf, CliError> {
    let key = auth.account_lock_key()?;
    let dir = crate::core::project_config_dir()
        .map(|dir| dir.join("locks"))
        .ok_or_else(|| CliError::Config("cannot resolve sunox config directory".into()))?;
    Ok(dir.join(format!("auth-refresh-{key}.lock")))
}
