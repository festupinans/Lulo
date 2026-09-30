//! Reads and writes `<status dir>/<session_id>.json`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::progress;
use crate::state::{self, Action};

/// `%LOCALAPPDATA%\claude-status` on Windows. `LULO_STATUS_DIR` overrides it,
/// and other platforms fall back to `~/.local/state/claude-status` so the hook
/// can be developed and tested anywhere.
pub fn status_dir() -> Option<PathBuf> {
    if let Some(dir) = env_path("LULO_STATUS_DIR") {
        return Some(dir);
    }
    if cfg!(windows) {
        return env_path("LOCALAPPDATA").map(|d| d.join("claude-status"));
    }
    env_path("XDG_STATE_HOME")
        .or_else(|| env_path("HOME").map(|h| h.join(".local").join("state")))
        .map(|d| d.join("claude-status"))
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Applies one hook event to the status directory.
pub fn apply(dir: &Path, input: &Value) -> io::Result<()> {
    let Some(session_id) = input.get("session_id").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(file) = session_file(dir, session_id) else {
        return Ok(());
    };

    let action = state::classify(input);
    if action == Action::Remove {
        return match fs::remove_file(&file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let prev = fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    match progress::merge(prev, input, &action, now_secs()) {
        Some(record) => write_atomic(dir, &file, &record),
        None => Ok(()),
    }
}

/// Session ids are UUIDs; anything else is rejected so a crafted id can't
/// escape the status directory.
fn session_file(dir: &Path, session_id: &str) -> Option<PathBuf> {
    let valid = !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| dir.join(format!("{session_id}.json")))
}

/// Writes to a temp file and renames it over the target, so the widget never
/// reads a half-written file.
fn write_atomic(dir: &Path, file: &Path, record: &Value) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let tmp = file.with_extension(format!("json.{}.tmp", std::process::id()));
    fs::write(&tmp, serde_json::to_vec(record)?)?;

    // On Windows the rename fails while the widget has the target open, so
    // retry briefly instead of losing the update.
    let mut result = fs::rename(&tmp, file);
    for _ in 0..5 {
        if result.is_ok() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
        result = fs::rename(&tmp, file);
    }
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
