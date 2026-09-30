//! Claude Code sessions that run in the cloud (claude.ai projects, the web,
//! the Code tab's cloud sessions) never run the hooks on this PC. This polls
//! the list of the account's sessions, the same one `claude --teleport`
//! shows, and writes one `cloud-<id>.json` per live session into the status
//! folder, in the hook's format, so the rest of the widget treats them like
//! any other session.
//!
//! The API only says whether a session is working, waiting on the user or
//! idle, so cloud sessions never show editing, bash and the like.
//!
//! It uses the login Claude Code saves in `~/.claude/.credentials.json`. The
//! token is only read, never refreshed or stored, and only sent to
//! Anthropic. If it expired, polling waits until Claude Code renews it.
//! This API is not documented by Anthropic and may stop working; cloud
//! sessions then simply don't show.

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

const URL: &str = "https://api.anthropic.com/v1/code/sessions";
const EVERY: Duration = Duration::from_secs(20);
/// Prefix of the files this module owns in the status folder.
const PREFIX: &str = "cloud-";

/// Starts the polling thread.
pub fn start(dir: PathBuf, forget_secs: u64) {
    thread::spawn(move || loop {
        if let Err(e) = refresh(&dir, forget_secs) {
            eprintln!("lulo: cloud sessions: {e}");
        }
        thread::sleep(EVERY);
    });
}

fn refresh(dir: &Path, forget_secs: u64) -> Result<(), String> {
    let now = now_secs();
    let token = token(now)?;
    let body = fetch(&token)?;
    let records = records(&body, now, forget_secs).ok_or("unexpected response")?;
    sync(dir, &records).map_err(|e| e.to_string())
}

/// Writes one file per record and deletes the cloud files no longer listed.
fn sync(dir: &Path, records: &[Value]) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut keep = HashSet::new();
    for rec in records {
        let Some(name) = rec["session_id"].as_str().and_then(file_name) else {
            continue;
        };
        let file = dir.join(&name);
        // Leave the file alone when nothing changed, so the watcher stays quiet.
        let same = fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .is_some_and(|old| &old == rec);
        if !same {
            write_atomic(&file, rec)?;
        }
        keep.insert(name);
    }
    // Sessions that ended, were archived or went quiet leave the list.
    if let Ok(entries) = fs::read_dir(dir) {
        for path in entries.flatten().map(|e| e.path()) {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if name.starts_with(PREFIX) && name.ends_with(".json") && !keep.contains(name) {
                let _ = fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

/// Deletes every cloud session file, for when cloud sessions are turned off.
pub fn clear(dir: &Path) {
    let _ = sync(dir, &[]);
}

/// Same rule as the hook: ids are plain, so a crafted one can't leave the folder.
fn file_name(id: &str) -> Option<String> {
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then(|| format!("{PREFIX}{id}.json"))
}

/// One status record per live cloud session in the response.
fn records(body: &str, now: u64, forget_secs: u64) -> Option<Vec<Value>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let list = v.get("data")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|s| record(s, now, forget_secs))
            .collect(),
    )
}

fn record(s: &Value, now: u64, forget_secs: u64) -> Option<Value> {
    let field = |k: &str| s.get(k).and_then(Value::as_str);
    let id = field("id")?;
    // Remote Control sessions run on this PC, where the hooks already report them.
    if field("environment_kind") == Some("bridge") || field("status") == Some("archived") {
        return None;
    }
    let state = match field("worker_status") {
        Some("running") => "thinking",
        Some("requires_action") => "waiting",
        Some("idle") => "done",
        _ => return None,
    };
    let last = field("last_event_at")
        .or_else(|| field("created_at"))
        .and_then(epoch)
        .unwrap_or(0);
    if now.saturating_sub(last) >= forget_secs {
        return None;
    }
    // A running session is alive right now even if its last event is old.
    let ts = if state == "thinking" { now } else { last };
    let title = field("title")
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("Nube");
    Some(json!({
        "session_id": id,
        "project": clip(title, 60),
        "cwd": "",
        "state": state,
        "detail": null,
        "event": "cloud",
        "cloud": true,
        "ts": ts,
    }))
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
    let url = std::env::var("LULO_CLOUD_URL").unwrap_or_else(|_| URL.to_string());
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--show-error",
        "--fail",
        "--max-time",
        "15",
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
        "header = \"Authorization: Bearer {token}\"\nheader = \"anthropic-version: 2023-06-01\"\n"
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

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
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

    const NOW: u64 = 1_790_806_200; // 2026-09-30T22:10:00Z

    #[test]
    fn parses_times() {
        assert_eq!(epoch("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch("2026-09-30T22:10:00.713411+00:00"), Some(NOW));
        assert_eq!(epoch("2026-09-30T17:10:00-05:00"), Some(NOW));
        assert_eq!(epoch("yesterday"), None);
    }

    #[test]
    fn maps_cloud_sessions_to_states() {
        let body = r#"{"data":[
            {"id":"cse_run","title":"Revisar hooks","worker_status":"running","environment_kind":"anthropic_cloud","last_event_at":"2026-09-30T21:00:00Z"},
            {"id":"cse_ask","title":"Widget","worker_status":"requires_action","last_event_at":"2026-09-30T22:09:00Z"},
            {"id":"cse_idle","title":"","worker_status":"idle","last_event_at":"2026-09-30T22:00:00Z"},
            {"id":"cse_rc","title":"En mi PC","worker_status":"running","environment_kind":"bridge","last_event_at":"2026-09-30T22:09:00Z"},
            {"id":"cse_old","title":"Vieja","worker_status":"idle","last_event_at":"2026-09-28T22:00:00Z"},
            {"id":"cse_arch","title":"Archivada","status":"archived","worker_status":"idle","last_event_at":"2026-09-30T22:00:00Z"},
            {"id":"cse_new","title":"Pendiente","last_event_at":"2026-09-30T22:00:00Z"}
        ]}"#;
        let recs = records(body, NOW, 12 * 3600).unwrap();
        let got: Vec<_> = recs
            .iter()
            .map(|r| {
                (
                    r["session_id"].as_str().unwrap(),
                    r["state"].as_str().unwrap(),
                    r["project"].as_str().unwrap(),
                    r["ts"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("cse_run", "thinking", "Revisar hooks", NOW),
                ("cse_ask", "waiting", "Widget", NOW - 60),
                ("cse_idle", "done", "Nube", NOW - 600),
            ]
        );
        assert!(records("<html>", NOW, 1).is_none());
    }

    #[test]
    fn writes_and_forgets_files() {
        let dir = std::env::temp_dir().join(format!("lulo-cloud-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("cloud-cse_gone.json"), "{}").unwrap();
        fs::write(dir.join("local-session.json"), "{}").unwrap();
        assert_eq!(file_name("../x"), None);

        let rec = record(
            &json!({"id":"cse_a","title":"A","worker_status":"idle","last_event_at":"2026-09-30T22:00:00Z"}),
            NOW,
            3600,
        )
        .unwrap();
        sync(&dir, &[rec]).unwrap();
        let text = fs::read_to_string(dir.join("cloud-cse_a.json")).unwrap();
        let parsed = crate::sessions::Session::parse(&text).unwrap();
        assert_eq!(
            (parsed.project.as_str(), parsed.state.as_str()),
            ("A", "done")
        );
        assert!(!dir.join("cloud-cse_gone.json").exists());
        assert!(dir.join("local-session.json").exists());
        fs::remove_dir_all(&dir).unwrap();
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
