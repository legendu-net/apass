//! Location of `apass`'s config directory and the files inside it.
//!
//! Mirrors `apass.py`'s `CONFIG_DIR`, `PATH_CONFIG`, and `PATH_PROMPTS`
//! (`apass.py:18-20`).

use std::path::{Path, PathBuf};

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Config {
    dir: PathBuf,
}

impl Config {
    /// Build a config rooted at an explicit directory. Tests use this to
    /// point at a temporary directory instead of the real `$HOME`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// `$HOME/.config/apass`, matching `Path.home() / ".config" / "apass"`.
    pub fn from_home() -> Result<Self, AppError> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or(AppError::HomeNotSet)?;
        Ok(Self::new(home.join(".config").join("apass")))
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn prompts_path(&self) -> PathBuf {
        self.dir.join("prompts.yml")
    }

    pub fn profile_path(&self) -> PathBuf {
        self.dir.join("profile.json")
    }
}

/// The current username, matching the environment-variable lookup order of
/// Python's `getpass.getuser()` (`apass.py:22`): `LOGNAME`, `USER`, `LNAME`,
/// `USERNAME`. Unlike `getpass.getuser()` this has no `pwd`-database
/// fallback, so it errors if none of those variables are set.
pub fn current_username() -> Result<String, AppError> {
    username_from(|key| std::env::var(key).ok())
}

fn username_from(lookup: impl Fn(&str) -> Option<String>) -> Result<String, AppError> {
    ["LOGNAME", "USER", "LNAME", "USERNAME"]
        .into_iter()
        .find_map(lookup)
        .filter(|s| !s.is_empty())
        .ok_or(AppError::UsernameNotSet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_joined_under_the_config_dir() {
        let config = Config::new("/home/alice/.config/apass");
        assert_eq!(
            config.prompts_path(),
            PathBuf::from("/home/alice/.config/apass/prompts.yml")
        );
        assert_eq!(
            config.profile_path(),
            PathBuf::from("/home/alice/.config/apass/profile.json")
        );
    }

    #[test]
    fn username_prefers_logname_over_user() {
        let name = username_from(|key| match key {
            "LOGNAME" => Some("alice".to_string()),
            "USER" => Some("bob".to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(name, "alice");
    }

    #[test]
    fn username_falls_back_through_the_list() {
        let name = username_from(|key| match key {
            "USERNAME" => Some("carol".to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(name, "carol");
    }

    #[test]
    fn username_errors_when_nothing_is_set() {
        assert!(username_from(|_| None).is_err());
    }
}
