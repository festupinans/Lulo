//! Plan usage for sessions that have no status line, such as the Code tab of
//! the Claude desktop app. A background thread asks Anthropic for the
//! account's usage every few minutes, with the login Claude Code saved on
//! this computer, the same way `/usage` does, and writes it to the
//! `_usage.json` the rings read.
//!
//! The endpoint is not documented, so any failure (no login saved, expired
//! token, changed response) just leaves the rings without data. The token is
//! only read, never refreshed or stored, and only sent to Anthropic.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::rings::USAGE_FILE;

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
const EVERY: Duration = Duration::from_secs(5 * 60);
/// Status line data this recent is fresher than asking; skip the request.
const STATUS_LINE_FRESH_SECS: u64 = 4 * 60;

/// Starts the polling thread. `on_update` runs after each successful write.
pub fn start(dir: PathBuf, on_update: impl Fn() + Send + 'static) {
    thread::spawn(move || loop {
        if let Err(e) = refresh(&dir) {
            eprintln!("lulo: plan usage: {e}");
        } else {
            on_update();
        }
        thread::sleep(EVERY);
    });
}

fn refresh(dir: &Path) -> Result<(), String> {
    let now = now_secs();
    let file = dir.join(USAGE_FILE);
    if let Some(ts) = fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("ts")?.as_u64())
    {
        if now.saturating_sub(ts) < STATUS_LINE_FRESH_SECS {
            return Ok(());
        }
    }
    let token = token(now)?;
    let body = fetch(&token)?;
    let record = record(&body, now).ok_or("unexpected response")?;
    write_atomic(&file, &record).map_err(|e| e.to_string())
}

/// `~/.claude/.credentials.json`, honouring `CLAUDE_CONFIG_DIR`.
fn credentials_path() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = env("CLAUDE_CONFIG_DIR").or_else(|| {
        env(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|h| h.join(".claude"))
    })?;
    Some(dir.join(".credentials.json"))
}

fn token(now: u64) -> Result<String, String> {
    let path = credentials_path().ok_or("no home folder")?;
    let text = fs::read_to_string(&path).map_err(|_| "no saved Claude login")?;
    parse_token(&text, now)
}

/// The OAuth access token, unless it has already expired (Claude Code
/// refreshes it the next time it runs).
fn parse_token(text: &str, now: u64) -> Result<String, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "unreadable login file")?;
    let oauth = v.get("claudeAiOauth").ok_or("no Claude account login")?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .ok_or("no access token")?;
    if let Some(expires_ms) = oauth.get("expiresAt").and_then(Value::as_u64) {
        if expires_ms / 1000 <= now {
            return Err("login expired; it renews when Claude Code runs".into());
        }
    }
    Ok(token.to_string())
}

/// Uses the system's curl (built into Windows 10 and 11). The token goes in
/// through stdin, so it never shows in the process list.
fn fetch(token: &str) -> Result<String, String> {
    let url = std::env::var("LULO_USAGE_URL").unwrap_or_else(|_| URL.to_string());
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "20",
        "--config",
        "-",
        &url,
    ])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up from a windowless app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("curl: {e}"))?;
    let config = format!(
        "header = \"Authorization: Bearer {token}\"\nheader = \"anthropic-beta: oauth-2025-04-20\"\n"
    );
    child
        .stdin
        .take()
        .ok_or("curl: no stdin")?
        .write_all(config.as_bytes())
        .map_err(|e| format!("curl: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "curl: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "response is not text".into())
}

/// Turns the response into the same record the status line writes:
/// `{ "five_hour": {used_percentage, resets_at}, "seven_day": {...}, "ts" }`.
fn record(body: &str, now: u64) -> Option<Value> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut rec = serde_json::Map::new();
    for window in ["five_hour", "seven_day"] {
        let Some(w) = v.get(window).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(used) = w.get("utilization").and_then(Value::as_f64) else {
            continue;
        };
        let mut entry = json!({ "used_percentage": used.clamp(0.0, 100.0) });
        if let Some(reset) = w.get("resets_at").and_then(Value::as_str).and_then(epoch) {
            entry["resets_at"] = json!(reset);
        }
        rec.insert(window.to_string(), entry);
    }
    if !rec.contains_key("five_hour") {
        return None;
    }
    rec.insert("ts".to_string(), json!(now));
    rec.insert("source".to_string(), json!("account"));
    Some(Value::Object(rec))
}

/// Unix seconds from an RFC 3339 time such as `2026-09-30T23:00:00.123+00:00`.
fn epoch(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    // Skip fractional seconds, then read the offset.
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "Z" | "z" | "" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
    };
    // Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + sec - offset).ok()
}

fn write_atomic(file: &Path, record: &Value) -> std::io::Result<()> {
    let tmp = file.with_extension("json.widget.tmp");
    fs::write(&tmp, serde_json::to_vec(record)?)?;
    fs::rename(&tmp, file).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_times() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2026-09-30T22:10:00+00:00"), Some(1_790_806_200));
        assert_eq!(
            epoch("2026-09-30T22:10:00.713411+00:00"),
            Some(1_790_806_200)
        );
        assert_eq!(epoch("2026-09-30T17:10:00-05:00"), Some(1_790_806_200));
        assert_eq!(epoch("yesterday"), None);
    }

    #[test]
    fn reads_the_usage_response() {
        let body = r#"{"five_hour":{"utilization":34.0,"resets_at":"2026-09-30T22:10:00.5+00:00"},
                       "seven_day":{"utilization":12.5,"resets_at":null},"seven_day_opus":null}"#;
        let r = record(body, 7).unwrap();
        assert_eq!(r["five_hour"]["used_percentage"], 34.0);
        assert_eq!(r["five_hour"]["resets_at"], 1_790_806_200u64);
        assert_eq!(r["seven_day"]["used_percentage"], 12.5);
        assert!(r["seven_day"].get("resets_at").is_none());
        assert_eq!(r["ts"], 7);
        assert!(record(r#"{"five_hour":null}"#, 7).is_none());
        assert!(record("<html>", 7).is_none());
    }

    #[test]
    fn skips_expired_logins() {
        let login = |expires: u64| {
            format!(r#"{{"claudeAiOauth":{{"accessToken":"tok","expiresAt":{expires}}}}}"#)
        };
        assert_eq!(parse_token(&login(2_000_000), 1_000).as_deref(), Ok("tok"));
        assert!(parse_token(&login(999_000), 1_000).is_err());
        assert!(parse_token(r#"{"other":{}}"#, 1_000).is_err());
    }
}
