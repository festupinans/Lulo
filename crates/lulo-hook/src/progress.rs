//! Builds the session record from the previous one plus a hook event: the
//! current state, the prompt being worked on, the steps taken for it, and
//! Claude's task list when it keeps one.
//!
//! Pure functions only, so the whole merge is unit-testable.

use serde_json::{json, Map, Value};

use crate::state::{self, Action};

/// Steps kept per prompt; older ones are dropped.
const MAX_STEPS: usize = 15;
const PROMPT_MAX_CHARS: usize = 240;

/// Returns the new record to write, or `None` to leave the file as it is.
pub fn merge(prev: Option<Value>, input: &Value, action: &Action, now: u64) -> Option<Value> {
    let event = str_field(input, "hook_event_name").unwrap_or("");
    let fresh_start = event == "SessionStart";
    let mut rec = match prev {
        Some(Value::Object(map)) if !fresh_start => map,
        // Events that only report activity or tasks never invent a session.
        _ if !matches!(action, Action::Write { .. }) => return None,
        _ => Map::new(),
    };

    let tasks_changed = update_tasks(&mut rec, input);
    match action {
        Action::Write { state, detail } => {
            let cwd = str_field(input, "cwd").unwrap_or("");
            rec.insert("session_id".into(), json!(str_field(input, "session_id")));
            rec.insert("project".into(), json!(state::file_name(cwd)));
            rec.insert("cwd".into(), json!(cwd));
            rec.insert("state".into(), json!(state.as_str()));
            rec.insert("detail".into(), json!(detail));
            rec.insert("event".into(), json!(event));

            if event == "UserPromptSubmit" {
                let prompt = str_field(input, "prompt")
                    .or_else(|| str_field(input, "user_prompt"))
                    .map(clip_prompt);
                rec.insert("prompt".into(), json!(prompt));
                rec.insert("started".into(), json!(now));
                rec.insert("steps".into(), json!([]));
            }
            if event == "PreToolUse" {
                push_step(&mut rec, state.as_str(), detail.as_deref(), now);
            }
        }
        Action::Touch => {}
        Action::Ignore if tasks_changed => {}
        Action::Ignore | Action::Remove => return None,
    }
    rec.insert("ts".into(), json!(now));
    Some(Value::Object(rec))
}

fn push_step(rec: &mut Map<String, Value>, state: &str, detail: Option<&str>, now: u64) {
    let steps = rec.entry("steps").or_insert_with(|| json!([]));
    if !steps.is_array() {
        *steps = json!([]);
    }
    let steps = steps.as_array_mut().expect("just made an array");
    steps.push(json!({ "state": state, "detail": detail, "ts": now }));
    if steps.len() > MAX_STEPS {
        steps.drain(..steps.len() - MAX_STEPS);
    }
}

/// Applies TodoWrite, TaskCreated, TaskUpdate and TaskCompleted to the
/// record's `tasks` list. Returns whether it changed.
fn update_tasks(rec: &mut Map<String, Value>, input: &Value) -> bool {
    let event = str_field(input, "hook_event_name").unwrap_or("");
    let tool = str_field(input, "tool_name").unwrap_or("");
    let tool_input = input.get("tool_input").unwrap_or(&Value::Null);

    let mut tasks: Vec<Value> = rec
        .get("tasks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let before = tasks.clone();

    match (event, tool) {
        // TodoWrite always sends the whole list.
        ("PreToolUse", "TodoWrite") => {
            if let Some(todos) = tool_input.get("todos").and_then(Value::as_array) {
                tasks = todos
                    .iter()
                    .filter_map(|t| {
                        let text =
                            str_field(t, "content").or_else(|| str_field(t, "activeForm"))?;
                        let status = str_field(t, "status").unwrap_or("pending");
                        Some(json!({ "id": null, "text": clip(text), "status": status }))
                    })
                    .collect();
            }
        }
        ("TaskCreated", _) => {
            if let (Some(id), Some(subject)) = (
                str_or_num(input, "task_id"),
                str_field(input, "task_subject"),
            ) {
                if !tasks.iter().any(|t| task_id(t) == Some(id.as_str())) {
                    tasks.push(json!({ "id": id, "text": clip(subject), "status": "pending" }));
                }
            }
        }
        ("TaskCompleted", _) => {
            if let Some(id) = str_or_num(input, "task_id") {
                set_status(&mut tasks, &id, "completed");
            }
        }
        ("PostToolUse", "TaskUpdate") => {
            let id = str_or_num(tool_input, "taskId").or_else(|| str_or_num(tool_input, "task_id"));
            if let Some(id) = id {
                match str_field(tool_input, "status") {
                    Some("deleted") => tasks.retain(|t| task_id(t) != Some(id.as_str())),
                    Some(status) => set_status(&mut tasks, &id, status),
                    None => {}
                }
                if let Some(subject) = str_field(tool_input, "subject") {
                    for t in tasks.iter_mut().filter(|t| task_id(t) == Some(id.as_str())) {
                        t["text"] = json!(clip(subject));
                    }
                }
            }
        }
        _ => {}
    }

    if tasks == before {
        return false;
    }
    rec.insert("tasks".into(), Value::Array(tasks));
    true
}

fn set_status(tasks: &mut [Value], id: &str, status: &str) {
    for t in tasks.iter_mut().filter(|t| task_id(t) == Some(id)) {
        t["status"] = json!(status);
    }
}

fn task_id(task: &Value) -> Option<&str> {
    task.get("id").and_then(Value::as_str)
}

/// Task ids arrive as strings ("task-001") or numbers depending on the tool.
fn str_or_num(v: &Value, key: &str) -> Option<String> {
    match v.get(key)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

fn clip(s: &str) -> String {
    clip_to(s.lines().next().unwrap_or("").trim(), 120)
}

fn clip_prompt(s: &str) -> String {
    let joined = s.split_whitespace().collect::<Vec<_>>().join(" ");
    clip_to(&joined, PROMPT_MAX_CHARS)
}

fn clip_to(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::classify;

    fn ev(name: &str, extra: Value) -> Value {
        let mut v = json!({ "session_id": "s1", "cwd": "/w/Lulo", "hook_event_name": name });
        v.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        v
    }

    fn step(prev: Option<Value>, input: Value, now: u64) -> Option<Value> {
        merge(prev, &input, &classify(&input), now)
    }

    #[test]
    fn prompt_resets_steps_and_tools_add_them() {
        let r = step(None, ev("SessionStart", json!({})), 1).unwrap();
        let r = step(
            Some(r),
            ev(
                "UserPromptSubmit",
                json!({ "prompt": "Arregla\n  el   login" }),
            ),
            2,
        )
        .unwrap();
        assert_eq!(r["prompt"], "Arregla el login");
        assert_eq!(r["started"], 2);
        let r = step(
            Some(r),
            ev(
                "PreToolUse",
                json!({ "tool_name": "Read", "tool_input": { "file_path": "/w/a.rs" } }),
            ),
            3,
        )
        .unwrap();
        let r = step(
            Some(r),
            ev("PostToolUse", json!({ "tool_name": "Read" })),
            4,
        )
        .unwrap();
        let r = step(
            Some(r),
            ev(
                "PreToolUse",
                json!({ "tool_name": "Edit", "tool_input": { "file_path": "/w/a.rs" } }),
            ),
            5,
        )
        .unwrap();
        let steps = r["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(
            steps[0],
            json!({ "state": "reading", "detail": "a.rs", "ts": 3 })
        );
        assert_eq!(steps[1]["state"], "editing");
        assert_eq!(r["state"], "editing");

        let r = step(
            Some(r),
            ev("UserPromptSubmit", json!({ "prompt": "Otra cosa" })),
            6,
        )
        .unwrap();
        assert_eq!(r["steps"], json!([]));
        assert_eq!(r["prompt"], "Otra cosa");
    }

    #[test]
    fn steps_are_capped() {
        let mut r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 0);
        for i in 0..40 {
            r = step(
                r,
                ev(
                    "PreToolUse",
                    json!({ "tool_name": "Bash", "tool_input": { "command": format!("c{i}") } }),
                ),
                i,
            );
        }
        let steps = r.unwrap()["steps"].as_array().unwrap().clone();
        assert_eq!(steps.len(), MAX_STEPS);
        assert_eq!(steps.last().unwrap()["detail"], "c39");
    }

    #[test]
    fn todo_write_replaces_the_list() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 0);
        let todos = json!({ "tool_name": "TodoWrite", "tool_input": { "todos": [
            { "content": "Leer código", "status": "completed", "activeForm": "Leyendo código" },
            { "content": "Arreglar bug", "status": "in_progress", "activeForm": "Arreglando bug" },
            { "content": "Probar", "status": "pending", "activeForm": "Probando" }
        ] } });
        let r = step(r, ev("PreToolUse", todos), 1).unwrap();
        let tasks = r["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(
            tasks[1],
            json!({ "id": null, "text": "Arreglar bug", "status": "in_progress" })
        );
    }

    #[test]
    fn task_tools_build_and_update_the_list() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 0);
        let r = step(
            r,
            ev(
                "TaskCreated",
                json!({ "task_id": "1", "task_subject": "Leer" }),
            ),
            1,
        );
        let r = step(
            r,
            ev(
                "TaskCreated",
                json!({ "task_id": "2", "task_subject": "Arreglar" }),
            ),
            2,
        );
        let r = step(
            r,
            ev(
                "TaskCreated",
                json!({ "task_id": "3", "task_subject": "Borrar" }),
            ),
            3,
        );
        let r = step(
            r,
            ev(
                "PostToolUse",
                json!({ "tool_name": "TaskUpdate", "tool_input": { "taskId": "2", "status": "in_progress" } }),
            ),
            4,
        );
        let r = step(
            r,
            ev(
                "TaskCompleted",
                json!({ "task_id": "1", "task_subject": "Leer" }),
            ),
            5,
        );
        let r = step(r, ev("PostToolUse", json!({ "tool_name": "TaskUpdate", "tool_input": { "taskId": 3, "status": "deleted" } })), 6).unwrap();
        assert_eq!(
            r["tasks"],
            json!([
                { "id": "1", "text": "Leer", "status": "completed" },
                { "id": "2", "text": "Arreglar", "status": "in_progress" }
            ])
        );
        // TaskCreated alone (an otherwise ignored event) still refreshed ts.
        assert_eq!(r["ts"], 6);
    }

    #[test]
    fn never_invents_a_session() {
        assert!(step(
            None,
            ev(
                "TaskCreated",
                json!({ "task_id": "1", "task_subject": "x" })
            ),
            1
        )
        .is_none());
        let sub = ev(
            "PreToolUse",
            json!({ "tool_name": "Bash", "agent_id": "a" }),
        );
        assert!(step(None, sub, 1).is_none());
        assert!(step(
            Some(json!({ "state": "done" })),
            ev("PreCompact", json!({})),
            1
        )
        .is_none());
    }

    #[test]
    fn subagent_steps_are_not_recorded() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 0);
        let r = step(
            r,
            ev(
                "PreToolUse",
                json!({ "tool_name": "Agent", "tool_input": { "description": "Explorar" } }),
            ),
            1,
        );
        let r = step(
            r,
            ev(
                "PreToolUse",
                json!({ "tool_name": "Bash", "agent_id": "a", "tool_input": { "command": "ls" } }),
            ),
            2,
        )
        .unwrap();
        assert_eq!(r["state"], "subagent");
        assert_eq!(r["steps"].as_array().unwrap().len(), 1);
        assert_eq!(r["ts"], 2);
    }

    #[test]
    fn session_start_clears_old_data() {
        let old = json!({ "state": "done", "prompt": "viejo", "tasks": [{ "id": "1", "text": "x", "status": "pending" }] });
        let r = step(
            Some(old),
            ev("SessionStart", json!({ "source": "clear" })),
            9,
        )
        .unwrap();
        assert!(r.get("prompt").is_none());
        assert!(r.get("tasks").is_none());
        assert_eq!(r["state"], "ready");
    }
}
