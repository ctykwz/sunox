use std::path::PathBuf;

pub(crate) fn project_config_dir() -> Option<PathBuf> {
    non_empty_env_path("XDG_CONFIG_HOME")
        .map(|dir| dir.join("sunox"))
        .or_else(|| {
            directories::ProjectDirs::from("com", "sunox", "sunox")
                .map(|dirs| dirs.config_dir().to_path_buf())
        })
}

pub(crate) fn user_home_dir() -> Option<PathBuf> {
    non_empty_env_path("HOME")
        .or_else(|| directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf()))
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::non_empty_env_path;

    #[test]
    fn missing_path_override_is_ignored() {
        assert_eq!(non_empty_env_path("SUNOX_TEST_MISSING_PATH"), None);
    }
}
