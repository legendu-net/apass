//! Cache and manage named passwords, keyed by name so more than one
//! password can be cached at once.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// The password name used by a prompt entry that doesn't specify one.
pub const DEFAULT_NAME: &str = "default";

const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.6f";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Token {
    password: String,
    time: String,
}

/// True for the old single-password shape: a `password`/`time` pair
/// directly at the top level, rather than nested under a name.
fn is_old_flat_format(value: &serde_json::Value) -> bool {
    value.get("password").is_some_and(|v| v.is_string())
        && value.get("time").is_some_and(|v| v.is_string())
}

/// The on-disk `profile.json` store: a name -> cached-password map.
#[derive(Debug)]
pub struct Profile {
    path: PathBuf,
    tokens: BTreeMap<String, Token>,
}

impl Profile {
    /// Load `profile.json`, treating a missing file as an empty store.
    pub fn load(path: &Path) -> Result<Self, AppError> {
        let tokens = if path.is_file() {
            let text = fs::read_to_string(path)?;
            // Parse generically first, so a genuinely malformed file (bad
            // syntax, truncated write) surfaces its real `serde_json::Error`
            // via `AppError::Json` below, rather than being misreported as
            // the old single-password format.
            let value: serde_json::Value = serde_json::from_str(&text)?;
            if is_old_flat_format(&value) {
                return Err(AppError::OldProfileFormat {
                    path: path.to_path_buf(),
                });
            }
            serde_json::from_value::<BTreeMap<String, Token>>(value)?
        } else {
            BTreeMap::new()
        };
        Ok(Self {
            path: path.to_path_buf(),
            tokens,
        })
    }

    /// Returns `Ok(Some(password))` when `name` is cached and still within
    /// `timeout`, `Ok(None)` when it's absent or has expired, and `Err` for
    /// a corrupt entry.
    pub fn password(&self, name: &str, timeout: Duration) -> Result<Option<String>, AppError> {
        let Some(token) = self.tokens.get(name) else {
            return Ok(None);
        };
        let cached_at = NaiveDateTime::parse_from_str(&token.time, TIME_FORMAT).map_err(|_| {
            AppError::InvalidCacheTimestamp {
                path: self.path.clone(),
                timestamp: token.time.clone(),
            }
        })?;

        // A negative age (clock skew, or the cache was just written) still
        // counts as fresh.
        let age_secs = (Local::now().naive_local() - cached_at).num_milliseconds() as f64 / 1000.0;
        if age_secs > timeout.as_secs_f64() {
            return Ok(None);
        }

        let decoded = STANDARD.decode(&token.password)?;
        let password = String::from_utf8(decoded).map_err(|_| AppError::InvalidCachedPassword {
            path: self.path.clone(),
        })?;
        Ok(Some(password))
    }

    /// Store (or overwrite) `name`, stamping it with the current time.
    pub fn set(&mut self, name: &str, password: &str) {
        self.tokens.insert(
            name.to_string(),
            Token {
                password: STANDARD.encode(password.as_bytes()),
                time: Local::now().naive_local().format(TIME_FORMAT).to_string(),
            },
        );
    }

    /// Remove `name`. Returns `true` if it was present.
    pub fn remove(&mut self, name: &str) -> bool {
        self.tokens.remove(name).is_some()
    }

    /// Names paired with their cached timestamp, sorted by name. Never
    /// exposes the password itself.
    pub fn names(&self) -> impl Iterator<Item = (&str, &str)> {
        self.tokens
            .iter()
            .map(|(name, token)| (name.as_str(), token.time.as_str()))
    }

    pub fn save(&self) -> Result<(), AppError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.path, serde_json::to_string_pretty(&self.tokens)?)?;
        Ok(())
    }
}

/// `apass password set [<name>]`: prompt (no echo) for a password and cache
/// it under `name`.
pub fn cmd_set(path: &Path, name: &str) -> Result<(), AppError> {
    set_with(path, name, || {
        rpassword::prompt_password(format!("Please enter the password for {name:?}: "))
            .map_err(AppError::from)
    })
}

/// Same as [`cmd_set`], but obtains the password from `read_password`
/// instead of always prompting on the real terminal, so tests can exercise
/// this without blocking on stdin.
fn set_with(
    path: &Path,
    name: &str,
    read_password: impl FnOnce() -> Result<String, AppError>,
) -> Result<(), AppError> {
    let mut profile = Profile::load(path)?;
    let password = read_password()?;
    profile.set(name, &password);
    profile.save()
}

/// `apass password list`: print each cached name and when it was cached.
/// Never prints a password.
pub fn cmd_list(path: &Path) -> Result<(), AppError> {
    let profile = Profile::load(path)?;
    for (name, time) in profile.names() {
        println!("{name}\t{time}");
    }
    Ok(())
}

/// `apass password remove <name>`.
pub fn cmd_remove(path: &Path, name: &str) -> Result<(), AppError> {
    let mut profile = Profile::load(path)?;
    if !profile.remove(name) {
        return Err(AppError::PasswordEntryNotFound {
            name: name.to_string(),
            path: path.to_path_buf(),
        });
    }
    profile.save()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(password: &str, time: NaiveDateTime) -> Token {
        Token {
            password: STANDARD.encode(password.as_bytes()),
            time: time.format(TIME_FORMAT).to_string(),
        }
    }

    #[test]
    fn writes_and_reads_back_the_same_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let mut tokens = BTreeMap::new();
        tokens.insert(
            DEFAULT_NAME.to_string(),
            token("hunter2", Local::now().naive_local()),
        );
        fs::write(&path, serde_json::to_string(&tokens).unwrap()).unwrap();

        let profile = Profile::load(&path).unwrap();
        assert_eq!(
            profile
                .password(DEFAULT_NAME, Duration::from_secs(3600))
                .unwrap(),
            Some("hunter2".to_string())
        );
    }

    #[test]
    fn multiple_names_round_trip_independently() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let mut profile = Profile::load(&path).unwrap();
        profile.set(DEFAULT_NAME, "hunter2");
        profile.set("work", "correct-horse");
        profile.save().unwrap();

        let profile = Profile::load(&path).unwrap();
        assert_eq!(
            profile
                .password(DEFAULT_NAME, Duration::from_secs(3600))
                .unwrap(),
            Some("hunter2".to_string())
        );
        assert_eq!(
            profile.password("work", Duration::from_secs(3600)).unwrap(),
            Some("correct-horse".to_string())
        );
    }

    #[test]
    fn unknown_name_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let profile = Profile::load(&path).unwrap();
        assert_eq!(
            profile.password("work", Duration::from_secs(3600)).unwrap(),
            None
        );
    }

    #[test]
    fn expired_entry_is_treated_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let an_hour_ago = Local::now().naive_local() - chrono::Duration::hours(1);
        let mut tokens = BTreeMap::new();
        tokens.insert(DEFAULT_NAME.to_string(), token("hunter2", an_hour_ago));
        fs::write(&path, serde_json::to_string(&tokens).unwrap()).unwrap();

        let profile = Profile::load(&path).unwrap();
        assert_eq!(
            profile
                .password(DEFAULT_NAME, Duration::from_secs(60))
                .unwrap(),
            None
        );
    }

    #[test]
    fn old_flat_profile_format_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        // The old single-password shape: `password`/`time` at the top
        // level rather than nested under a name.
        fs::write(
            &path,
            "{\n  \"password\": \"aHVudGVyMg==\",\n  \"time\": \"2026-01-01 00:00:00.000000\"\n}",
        )
        .unwrap();

        let err = Profile::load(&path).unwrap_err();
        assert!(matches!(err, AppError::OldProfileFormat { .. }));
    }

    #[test]
    fn corrupt_profile_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        fs::write(&path, "not json").unwrap();
        assert!(Profile::load(&path).is_err());
    }

    #[test]
    fn corrupt_profile_is_not_mistaken_for_the_old_format() {
        // Malformed JSON should surface as a plain parse error, not be
        // misreported as the old single-password shape.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        fs::write(&path, "not json at all {{{").unwrap();
        let err = Profile::load(&path).unwrap_err();
        assert!(!matches!(err, AppError::OldProfileFormat { .. }));
    }

    #[test]
    fn set_overwrites_and_refreshes_the_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let an_hour_ago = Local::now().naive_local() - chrono::Duration::hours(1);
        let mut tokens = BTreeMap::new();
        tokens.insert(DEFAULT_NAME.to_string(), token("old-password", an_hour_ago));
        fs::write(&path, serde_json::to_string(&tokens).unwrap()).unwrap();

        set_with(&path, DEFAULT_NAME, || Ok("new-password".to_string())).unwrap();

        let profile = Profile::load(&path).unwrap();
        assert_eq!(
            profile
                .password(DEFAULT_NAME, Duration::from_secs(60))
                .unwrap(),
            Some("new-password".to_string())
        );
    }

    #[test]
    fn cmd_remove_errors_when_the_name_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let err = cmd_remove(&path, "work").unwrap_err();
        assert!(matches!(err, AppError::PasswordEntryNotFound { .. }));
    }

    #[test]
    fn cmd_remove_deletes_an_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let mut profile = Profile::load(&path).unwrap();
        profile.set("work", "correct-horse");
        profile.save().unwrap();

        cmd_remove(&path, "work").unwrap();

        let profile = Profile::load(&path).unwrap();
        assert_eq!(
            profile.password("work", Duration::from_secs(3600)).unwrap(),
            None
        );
    }

    #[test]
    fn names_lists_every_cached_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let mut profile = Profile::load(&path).unwrap();
        profile.set("work", "correct-horse");
        profile.set(DEFAULT_NAME, "hunter2");

        let names: Vec<&str> = profile.names().map(|(name, _)| name).collect();
        assert_eq!(names, vec![DEFAULT_NAME, "work"]);
    }
}
