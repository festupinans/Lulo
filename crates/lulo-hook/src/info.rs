//! Facts about a session that the widget shows in its detail panel: the
//! model, the session title, the permission mode and effort, the last call
//! auto mode denied and the question an MCP server is asking.
//!
//! Pure functions only, like `progress`, so it is unit-testable.

use serde_json::{json, Map, Value};

use crate::state;

/// Copies what `input` says about the session into the record. Returns
/// whether anything changed.
pub fn update(rec: &mut Map<String, Value>, input: &Value, now: u64) -> bool {
    let before = rec.clone();
    let event = str_field(input, "hook_event_name").unwrap_or("");

    // Most events carry the current mode, so a change mid-session shows.
    if let Some(mode) = str_field(input, "permission_mode").filter(|m| !m.is_empty()) {
        rec.insert("permission_mode".into(), json!(mode));
    }
    if let Some(effort) = input.get("effort").and_then(effort_level) {
        rec.insert("effort".into(), json!(effort));
    }

    match event {
        "SessionStart" => {
            if let Some(model) = input.get("model").and_then(model_name) {
                rec.insert("model".into(), json!(model));
            }
            if let Some(title) = str_field(input, "session_title").filter(|t| !t.is_empty()) {
                rec.insert("title".into(), json!(clip(title)));
            }
        }
        "PostModelSwitch" => {
            if let Some(model) = input.get("to_model").and_then(model_name) {
                rec.insert("model".into(), json!(model));
            }
        }
        "PermissionDenied" => {
            let tool = str_field(input, "tool_name").unwrap_or("");
            let detail = state::tool_detail(tool, input.get("tool_input")).filter(|d| d != tool);
            rec.insert(
                "denied".into(),
                json!({ "tool": tool, "detail": detail, "ts": now }),
            );
        }
        "Elicitation" => {
            let server = str_field(input, "server_name").unwrap_or("");
            rec.insert(
                "question".into(),
                json!({ "server": server, "text": elicitation_text(input), "ts": now }),
            );
        }
        // The person wrote again: an old denial no longer needs attention.
        "UserPromptSubmit" if !is_task_notification(input) => {
            rec.remove("denied");
        }
        _ => {}
    }
    *rec != before
}

/// What the MCP server asks, from its message or its form.
pub fn elicitation_text(input: &Value) -> Option<String> {
    let form = input
        .get("requested_schema")
        .or_else(|| input.get("form_schema"));
    str_field(input, "message")
        .or_else(|| form.and_then(|f| str_field(f, "description")))
        .or_else(|| form.and_then(|f| str_field(f, "title")))
        .filter(|t| !t.trim().is_empty())
        .map(clip)
}

/// The model as Claude Code names it: a plain id, or an object with one.
fn model_name(v: &Value) -> Option<String> {
    let name = match v {
        Value::String(s) => Some(s.as_str()),
        Value::Object(o) => ["display_name", "id", "name"]
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_str)),
        _ => None,
    }?;
    Some(name.trim().to_string()).filter(|n| !n.is_empty())
}

fn effort_level(v: &Value) -> Option<String> {
    let level = match v {
        Value::String(s) => Some(s.as_str()),
        Value::Object(o) => ["level", "value"]
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_str)),
        _ => None,
    }?;
    Some(level.trim().to_string()).filter(|l| !l.is_empty())
}

fn is_task_notification(input: &Value) -> bool {
    str_field(input, "prompt").is_some_and(|p| p.trim_start().starts_with("<task-notification>"))
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

fn clip(s: &str) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 160 {
        return one_line;
    }
    let mut out: String = one_line.chars().take(159).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(rec: &mut Map<String, Value>, input: Value) -> bool {
        update(rec, &input, 50)
    }

    #[test]
    fn session_start_names_model_and_title() {
        let mut r = Map::new();
        assert!(run(
            &mut r,
            json!({ "hook_event_name": "SessionStart", "model": "claude-opus-4-8",
                    "session_title": "Arreglar el login", "permission_mode": "auto" }),
        ));
        assert_eq!(r["model"], "claude-opus-4-8");
        assert_eq!(r["title"], "Arreglar el login");
        assert_eq!(r["permission_mode"], "auto");

        run(
            &mut r,
            json!({ "hook_event_name": "PostModelSwitch", "to_model": { "id": "claude-sonnet-4-6" } }),
        );
        assert_eq!(r["model"], "claude-sonnet-4-6");
        // Nothing new: no change reported.
        assert!(!run(
            &mut r,
            json!({ "hook_event_name": "PreToolUse", "permission_mode": "auto" })
        ));
    }

    #[test]
    fn effort_as_text_or_object() {
        let mut r = Map::new();
        run(
            &mut r,
            json!({ "hook_event_name": "Stop", "effort": "high" }),
        );
        assert_eq!(r["effort"], "high");
        run(
            &mut r,
            json!({ "hook_event_name": "PreToolUse", "effort": { "level": "max" } }),
        );
        assert_eq!(r["effort"], "max");
    }

    #[test]
    fn denials_last_until_the_next_prompt() {
        let mut r = Map::new();
        run(
            &mut r,
            json!({ "hook_event_name": "PermissionDenied", "tool_name": "Bash",
                    "tool_input": { "command": "rm -rf build" } }),
        );
        assert_eq!(
            r["denied"],
            json!({ "tool": "Bash", "detail": "rm -rf build", "ts": 50 })
        );
        // A background task reporting back is not the person writing.
        run(
            &mut r,
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": "<task-notification>x" }),
        );
        assert!(r.contains_key("denied"));
        run(
            &mut r,
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": "sigue" }),
        );
        assert!(!r.contains_key("denied"));
    }

    #[test]
    fn elicitation_keeps_server_and_question() {
        let mut r = Map::new();
        run(
            &mut r,
            json!({ "hook_event_name": "Elicitation", "server_name": "github",
                    "message": "Elige el repositorio" }),
        );
        assert_eq!(r["question"]["server"], "github");
        assert_eq!(r["question"]["text"], "Elige el repositorio");
        let form = json!({ "hook_event_name": "Elicitation", "server_name": "x",
                           "form_schema": { "title": "Datos de acceso" } });
        assert_eq!(elicitation_text(&form).as_deref(), Some("Datos de acceso"));
    }
}
