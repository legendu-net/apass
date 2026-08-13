//! Cache and prompt for the password. Mirrors `apass.py::_get_password`
//! (`apass.py:69-83`).

use std::fs;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Same strftime format as `apass.py`'s `FORMAT = "%Y-%m-%d %H:%M:%S.%f"`,
/// so `profile.json` stays interchangeable between the two implementations.
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S%.6f";

#[derive(Debug, Serialize, Deserialize)]
struct Token {
    password: String,
    time: String,
}

/// Return the cached password if it is younger than `timeout`, otherwise
/// prompt for one (via `prompt_text`) and cache it.
pub fn get(path: &Path, timeout: Duration, prompt_text: &str) -> Result<String, AppError> {
    get_with(path, timeout, || {
        rpassword::prompt_password(prompt_text).map_err(AppError::from)
    })
}

/// Same as [`get`], but obtains an uncached password from `read_password`
/// instead of always prompting on the real terminal. Split out so tests can
/// exercise the first-run/cache-miss path (writing a fresh cache entry)
/// without blocking on stdin.
fn get_with(
    path: &Path,
    timeout: Duration,
    read_password: impl FnOnce() -> Result<String, AppError>,
) -> Result<String, AppError> {
    if path.is_file()
        && let Some(password) = read_cached(path, timeout)?
    {
        return Ok(password);
    }

    let now = Local::now().naive_local();
    let password = read_password()?;
    let token = Token {
        password: STANDARD.encode(password.as_bytes()),
        time: now.format(TIME_FORMAT).to_string(),
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&token)?)?;
    Ok(password)
}

/// Returns `Ok(Some(password))` when the cache is present and still within
/// `timeout`, `Ok(None)` when it has expired, and `Err` for a corrupt cache.
fn read_cached(path: &Path, timeout: Duration) -> Result<Option<String>, AppError> {
    let text = fs::read_to_string(path)?;
    let token: Token = serde_json::from_str(&text)?;
    let cached_at = NaiveDateTime::parse_from_str(&token.time, TIME_FORMAT).map_err(|_| {
        AppError::InvalidCacheTimestamp {
            path: path.to_path_buf(),
            timestamp: token.time.clone(),
        }
    })?;

    // Match Python's `(datetime.now() - time_token).total_seconds() <=
    // timeout`: a negative age (clock skew, or the cache was just written)
    // still counts as fresh.
    let age_secs = (Local::now().naive_local() - cached_at).num_milliseconds() as f64 / 1000.0;
    if age_secs > timeout.as_secs_f64() {
        return Ok(None);
    }

    let decoded = STANDARD.decode(&token.password)?;
    let password = String::from_utf8(decoded).map_err(|_| AppError::InvalidCachedPassword {
        path: path.to_path_buf(),
    })?;
    Ok(Some(password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_and_reads_back_the_same_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        // `rpassword::prompt_password` would block on stdin, so exercise
        // the cache directly instead of going through `get`.
        let now = Local::now().naive_local();
        let token = Token {
            password: STANDARD.encode(b"hunter2"),
            time: now.format(TIME_FORMAT).to_string(),
        };
        fs::write(&path, serde_json::to_string(&token).unwrap()).unwrap();

        let cached = read_cached(&path, Duration::from_secs(3600)).unwrap();
        assert_eq!(cached, Some("hunter2".to_string()));
    }

    #[test]
    fn expired_cache_is_treated_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let an_hour_ago = Local::now().naive_local() - chrono::Duration::hours(1);
        let token = Token {
            password: STANDARD.encode(b"hunter2"),
            time: an_hour_ago.format(TIME_FORMAT).to_string(),
        };
        fs::write(&path, serde_json::to_string(&token).unwrap()).unwrap();

        let cached = read_cached(&path, Duration::from_secs(60)).unwrap();
        assert_eq!(cached, None);
    }

    #[test]
    fn parses_a_cache_written_by_the_python_version() {
        // Same shape `apass.py::_get_password` writes: base64 password,
        // "%Y-%m-%d %H:%M:%S.%f" timestamp.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let now = Local::now().naive_local();
        let py_time = now.format("%Y-%m-%d %H:%M:%S%.6f").to_string();
        let json = format!(
            "{{\n    \"password\": \"{}\",\n    \"time\": \"{}\"\n}}",
            STANDARD.encode(b"hunter2"),
            py_time
        );
        fs::write(&path, json).unwrap();

        let cached = read_cached(&path, Duration::from_secs(3600)).unwrap();
        assert_eq!(cached, Some("hunter2".to_string()));
    }

    #[test]
    fn corrupt_cache_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        fs::write(&path, "not json").unwrap();
        assert!(read_cached(&path, Duration::from_secs(3600)).is_err());
    }

    #[test]
    fn first_run_asks_for_a_password_and_caches_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        assert!(!path.is_file());

        let password = get_with(&path, Duration::from_secs(3600), || {
            Ok("hunter2".to_string())
        })
        .unwrap();
        assert_eq!(password, "hunter2");

        // The freshly written cache is itself readable, and round-trips.
        let cached = read_cached(&path, Duration::from_secs(3600)).unwrap();
        assert_eq!(cached, Some("hunter2".to_string()));
    }

    #[test]
    fn an_expired_cache_is_refreshed_instead_of_reused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let an_hour_ago = Local::now().naive_local() - chrono::Duration::hours(1);
        let stale = Token {
            password: STANDARD.encode(b"old-password"),
            time: an_hour_ago.format(TIME_FORMAT).to_string(),
        };
        fs::write(&path, serde_json::to_string(&stale).unwrap()).unwrap();

        let password = get_with(&path, Duration::from_secs(60), || {
            Ok("new-password".to_string())
        })
        .unwrap();
        assert_eq!(password, "new-password");
        assert_eq!(
            read_cached(&path, Duration::from_secs(3600)).unwrap(),
            Some("new-password".to_string())
        );
    }

    #[test]
    fn a_cache_hit_never_calls_read_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let now = Local::now().naive_local();
        let token = Token {
            password: STANDARD.encode(b"hunter2"),
            time: now.format(TIME_FORMAT).to_string(),
        };
        fs::write(&path, serde_json::to_string(&token).unwrap()).unwrap();

        let password = get_with(&path, Duration::from_secs(3600), || {
            panic!("should not prompt on a cache hit")
        })
        .unwrap();
        assert_eq!(password, "hunter2");
    }
}
