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
    pub(crate) async fn acquire(auth: &AuthState) -> Result<Self, CliError> {
        let path = lock_file_path(auth)?;
        Self::acquire_path(&path).await
    }

    async fn acquire_path(path: &Path) -> Result<Self, CliError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(Self { file }),
                Err(error)
                    if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {}
                Err(error) => return Err(error.into()),
            }
            if !crate::core::sleep_before_deadline(deadline, std::time::Duration::from_millis(25))
                .await
            {
                return Err(CliError::Config(
                    "timed out waiting for the account authentication refresh lock".into(),
                ));
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn contended_refresh_lock_yields_until_the_owner_releases_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("refresh.lock");
        let owner = AuthRefreshLockGuard::acquire_path(&path).await.unwrap();
        let release = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(owner);
        };
        let (acquired, ()) = tokio::join!(
            tokio::time::timeout(
                Duration::from_secs(2),
                AuthRefreshLockGuard::acquire_path(&path)
            ),
            release,
        );
        assert!(acquired.unwrap().is_ok());
    }

    #[tokio::test]
    async fn cancelling_a_refresh_lock_waiter_does_not_acquire_or_leak_the_lock() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("refresh.lock");
        let owner = AuthRefreshLockGuard::acquire_path(&path).await.unwrap();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(50),
                AuthRefreshLockGuard::acquire_path(&path)
            )
            .await
            .is_err()
        );
        drop(owner);
        let acquired = tokio::time::timeout(
            Duration::from_secs(1),
            AuthRefreshLockGuard::acquire_path(&path),
        )
        .await
        .unwrap()
        .unwrap();
        drop(acquired);
        let probe = File::options().read(true).write(true).open(&path).unwrap();
        probe.try_lock_exclusive().unwrap();
        FileExt::unlock(&probe).unwrap();
    }
}
