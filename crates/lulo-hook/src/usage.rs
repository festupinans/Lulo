//! Status line mode: Claude Code pipes the session's status JSON in, we save
//! the plan usage limits for the widget's rings and print a short line.
//!
//! Only the status line carries `rate_limits` (hooks don't). It is present for
//! Pro/Max plans, and only after the session's first API response.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::status;

/// The widget reads this name; the leading `_` keeps it apart from session files.
pub const USAGE_FILE: &str = "_usage.json";

/// Saves the usage limits, if the input has any, and returns the line to print.
pub fn apply(dir: &Path, input: &Value) -> String {
    let now = now_secs();
    if let Some(record) = usage_record(input, now) {
        if let Err(e) = status::write_atomic(dir, &dir.join(USAGE_FILE), &record) {
            eprintln!("lulo-hook: {e}");
        }
    }
    line(input, now)
}

/// `{ "five_hour": {used_percentage, resets_at}, "seven_day": {...}, "ts" }`,
/// keeping only the windows the input carries.
pub fn usage_record(input: &Value, now: u64) -> Option<Value> {
    let limits = input.get("rate_limits")?;
    let mut record = serde_json::Map::new();
    for window in ["five_hour", "seven_day"] {
        let Some(w) = limits.get(window) else {
            continue;
        };
        let Some(used) = w.get("used_percentage").and_then(Value::as_f64) else {
            continue;
        };
        let mut entry = json!({ "used_percentage": used.clamp(0.0, 100.0) });
        if let Some(reset) = w.get("resets_at").and_then(Value::as_u64) {
            entry["resets_at"] = json!(reset);
        }
        record.insert(window.to_string(), entry);
    }
    if record.is_empty() {
        return None;
    }
    record.insert("ts".to_string(), json!(now));
    Some(Value::Object(record))
}

/// `Opus · Lulo · 5 h: 34 % usado, reinicia en 2 h 10 min`
pub fn line(input: &Value, now: u64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(model) = input.pointer("/model/display_name").and_then(Value::as_str) {
        parts.push(model.to_string());
    }
    if let Some(dir) = input
        .pointer("/workspace/current_dir")
        .or_else(|| input.get("cwd"))
        .and_then(Value::as_str)
    {
        parts.push(crate::state::file_name(dir).to_string());
    }
    if let Some(w) = input.pointer("/rate_limits/five_hour") {
        if let Some(used) = w.get("used_percentage").and_then(Value::as_f64) {
            let mut s = format!("5 h: {:.0} % usado", used.clamp(0.0, 100.0));
            if let Some(reset) = w.get("resets_at").and_then(Value::as_u64) {
                if reset > now {
                    s.push_str(&format!(", reinicia en {}", duration(reset - now)));
                }
            }
            parts.push(s);
        }
    }
    parts.join(" · ")
}

fn duration(secs: u64) -> String {
    let mins = secs.div_ceil(60);
    match (mins / 60, mins % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "model": { "display_name": "Opus" },
            "workspace": { "current_dir": "C:\\dev\\Lulo" },
            "rate_limits": {
                "five_hour": { "used_percentage": 34.4, "resets_at": 1_000_000 + 7_800 },
                "seven_day": { "used_percentage": 12, "resets_at": 1_500_000 }
            }
        })
    }

    #[test]
    fn keeps_both_windows() {
        let r = usage_record(&sample(), 1_000_000).unwrap();
        assert_eq!(r["five_hour"]["used_percentage"], 34.4);
        assert_eq!(r["five_hour"]["resets_at"], 1_007_800);
        assert_eq!(r["seven_day"]["used_percentage"], 12.0);
        assert_eq!(r["ts"], 1_000_000);
    }

    #[test]
    fn no_limits_means_no_record() {
        assert!(usage_record(&json!({ "model": {} }), 1).is_none());
        assert!(usage_record(&json!({ "rate_limits": {} }), 1).is_none());
    }

    #[test]
    fn prints_a_short_line() {
        assert_eq!(
            line(&sample(), 1_000_000),
            "Opus · Lulo · 5 h: 34 % usado, reinicia en 2 h 10 min"
        );
        assert_eq!(line(&json!({ "cwd": "/home/me/api" }), 0), "api");
    }

    #[test]
    fn writes_the_usage_file() {
        let dir = std::env::temp_dir().join(format!("lulo-usage-{}", std::process::id()));
        apply(&dir, &sample());
        let text = std::fs::read_to_string(dir.join(USAGE_FILE)).unwrap();
        let saved: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["five_hour"]["used_percentage"], 34.4);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
