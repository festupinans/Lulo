//! The model and title of a session, read from the end of its transcript.
//! Hook events don't always carry them: `SessionStart` names the model only
//! in some Claude Code versions, and a session that was already open when
//! Lulo was installed never sends one.
//!
//! Only the last `TAIL` bytes are read, and only on events that happen once
//! per turn, so the hook stays fast.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use serde_json::{json, Value};

const TAIL: u64 = 256 * 1024;

/// Adds `lulo_model` and `lulo_title` to `input` when its transcript names
/// them, for `info::update` to store.
pub fn enrich(input: &mut Value) {
    let event = input
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if !matches!(event, "SessionStart" | "UserPromptSubmit" | "Stop") {
        return;
    }
    let Some(path) = input.get("transcript_path").and_then(Value::as_str) else {
        return;
    };
    let Some(tail) = read_tail(path) else {
        return;
    };
    let (model, title) = scan(&tail);
    if let Some(obj) = input.as_object_mut() {
        if let Some(m) = model {
            obj.insert("lulo_model".into(), json!(m));
        }
        if let Some(t) = title {
            obj.insert("lulo_title".into(), json!(t));
        }
    }
}

fn read_tail(path: &str) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The latest model that answered and the latest title, from transcript
/// lines (one JSON object per line; a line cut by the tail is skipped).
pub fn scan(tail: &str) -> (Option<String>, Option<String>) {
    let (mut model, mut title) = (None, None);
    for line in tail.lines().rev() {
        if model.is_some() && title.is_some() {
            break;
        }
        // Cheap filter before parsing: most lines are tool output.
        let maybe_model = model.is_none() && line.contains("\"model\"");
        let maybe_title =
            title.is_none() && (line.contains("itle\"") || line.contains("\"summary\""));
        if !maybe_model && !maybe_title {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
        if model.is_none() && kind == "assistant" {
            model = v
                .pointer("/message/model")
                .and_then(Value::as_str)
                // Messages Claude Code writes itself, not the model.
                .filter(|m| m.starts_with("claude"))
                .map(str::to_string);
        }
        if title.is_none() {
            let found = match kind {
                "custom-title" => v.get("customTitle"),
                "ai-title" => v.get("aiTitle").or_else(|| v.get("title")),
                "summary" => v.get("summary"),
                _ => None,
            };
            title = found
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_string);
        }
    }
    (model, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_latest_model_and_title() {
        let tail = r#"cut line without its start"}
{"type":"summary","summary":"Arreglar el login","leafUuid":"x"}
{"type":"assistant","message":{"model":"claude-sonnet-4-6","content":[]}}
{"type":"user","message":{"content":"sigue"}}
{"type":"assistant","message":{"model":"claude-opus-5-5","content":[]}}
{"type":"assistant","message":{"model":"<synthetic>","content":[]}}
"#;
        let (model, title) = scan(tail);
        assert_eq!(model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(title.as_deref(), Some("Arreglar el login"));
    }

    #[test]
    fn a_renamed_session_wins_over_the_summary() {
        let tail = r#"{"type":"summary","summary":"Viejo"}
{"type":"custom-title","customTitle":"Nuevo nombre","sessionId":"a"}
"#;
        assert_eq!(scan(tail).1.as_deref(), Some("Nuevo nombre"));
        assert_eq!(scan("").0, None);
    }
}
