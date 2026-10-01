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
/// Background tasks remembered at most; a leak can't grow the file forever.
const MAX_BACKGROUND: usize = 20;
/// Tools that stop a background shell.
const STOP_TOOLS: &[&str] = &["KillShell", "KillBash", "TaskStop"];

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

    let tasks_changed = update_tasks(&mut rec, input) | update_background(&mut rec, input);
    match action {
        Action::Write { state, detail } => {
            let prev_state = rec.get("state").cloned();
            rec.insert("session_id".into(), json!(str_field(input, "session_id")));
            // Each event carries the shell's current folder, which follows a
            // `cd`. The project is the folder the session started in.
            if !rec.contains_key("cwd") {
                let cwd = str_field(input, "cwd").unwrap_or("");
                rec.insert("project".into(), json!(state::file_name(cwd)));
                rec.insert("cwd".into(), json!(cwd));
            }
            rec.insert("state".into(), json!(state.as_str()));
            rec.insert("detail".into(), json!(detail));
            // The turn ended but shells or subagents it launched still run:
            // the session isn't done until they report back.
            let pending = background(&rec);
            if event == "Stop" && !pending.is_empty() {
                let first = pending[0].get("detail").cloned().unwrap_or(Value::Null);
                rec.insert("state".into(), json!("background"));
                rec.insert("detail".into(), first);
            }
            rec.insert("event".into(), json!(event));
            // When the shown state began, so the widget can say for how long.
            if rec.get("state") != prev_state.as_ref() || !rec.contains_key("since") {
                rec.insert("since".into(), json!(now));
            }

            // A background task reporting back arrives as a prompt too, but
            // the person's prompt and its steps are still the ones to show.
            if event == "UserPromptSubmit" && !is_task_notification(input) {
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

fn background(rec: &Map<String, Value>) -> Vec<Value> {
    rec.get("background")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Tracks the shells, monitors and subagents the main agent left running in
/// the background. Each entry keeps the `tool_use_id` that started it and,
/// once the tool answers, the task id Claude Code gave it. Claude Code tells
/// the model a task ended with a `<task-notification>` prompt naming both,
/// which removes the entry; stopping it with TaskStop does too. Returns
/// whether the list changed.
fn update_background(rec: &mut Map<String, Value>, input: &Value) -> bool {
    let event = str_field(input, "hook_event_name").unwrap_or("");
    let tool = str_field(input, "tool_name").unwrap_or("");
    let tool_input = input.get("tool_input").unwrap_or(&Value::Null);
    let in_subagent = input.get("agent_id").is_some_and(|v| !v.is_null());
    let mut list = background(rec);
    let before = list.clone();
    let field = |b: &Value, k: &str| b.get(k).and_then(Value::as_str).map(str::to_string);
    let drop_first = |list: &mut Vec<Value>, kind: &str| {
        if let Some(i) = list
            .iter()
            .position(|b| b.get("kind").and_then(Value::as_str) == Some(kind))
        {
            list.remove(i);
        }
    };
    // Drops the entry whose `id` or `task` is one of `keys`; whether any was.
    let drop_matching = |list: &mut Vec<Value>, keys: &[&str]| {
        let len = list.len();
        list.retain(|b| {
            !keys.iter().any(|k| {
                field(b, "id").as_deref() == Some(k) || field(b, "task").as_deref() == Some(k)
            })
        });
        list.len() != len
    };
    let kind_of = |tool: &str| match state::tool_state(tool) {
        state::State::Subagent => "agent",
        _ if tool == "Monitor" => "monitor",
        _ => "shell",
    };
    let tool_use_id = str_field(input, "tool_use_id");

    match event {
        // Monitor always runs in the background.
        "PreToolUse"
            if !in_subagent
                && (tool == "Monitor"
                    || tool_input.get("run_in_background").and_then(Value::as_bool)
                        == Some(true)) =>
        {
            let detail = str_field(tool_input, "description")
                .or_else(|| str_field(tool_input, "command"))
                .map(clip);
            list.push(json!({ "id": tool_use_id, "kind": kind_of(tool), "detail": detail }));
        }
        "PostToolUse" if !in_subagent => {
            let response = input.get("tool_response").unwrap_or(&Value::Null);
            let task = ["backgroundTaskId", "taskId", "task_id", "agentId"]
                .iter()
                .find_map(|k| str_field(response, k));
            let started = list
                .iter_mut()
                .find(|b| tool_use_id.is_some() && field(b, "id").as_deref() == tool_use_id);
            match (started, task) {
                (Some(entry), Some(task)) => entry["task"] = json!(task),
                // A command that outlived its timeout, or that the person
                // sent to the background, keeps running without having asked.
                (None, Some(task))
                    if tool != "TaskStop" && response.get("backgroundTaskId").is_some() =>
                {
                    let detail = str_field(tool_input, "description")
                        .or_else(|| str_field(tool_input, "command"))
                        .map(clip);
                    list.push(json!({ "id": tool_use_id, "task": task,
                                      "kind": kind_of(tool), "detail": detail }));
                }
                _ => {}
            }
        }
        // The tool failed, so nothing was left running.
        "PostToolUseFailure" if !in_subagent => {
            if let Some(id) = tool_use_id {
                drop_matching(&mut list, &[id]);
            }
        }
        "PreToolUse" if STOP_TOOLS.contains(&tool) => {
            let target =
                str_field(tool_input, "task_id").or_else(|| str_field(tool_input, "shell_id"));
            if !target.is_some_and(|t| drop_matching(&mut list, &[t])) {
                drop_first(&mut list, "shell");
            }
        }
        "UserPromptSubmit" => {
            for (keys, ended) in task_notifications(input) {
                if ended {
                    let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
                    drop_matching(&mut list, &keys);
                }
            }
        }
        "SubagentStop" => {
            let agent: Vec<&str> = str_field(input, "agent_id").into_iter().collect();
            // A foreground subagent finishing must not take a background one.
            if !drop_matching(&mut list, &agent) {
                if let Some(i) = list.iter().position(|b| {
                    field(b, "kind").as_deref() == Some("agent") && b.get("task").is_none()
                }) {
                    list.remove(i);
                }
            }
        }
        // Older Claude Code versions without task notifications.
        "Notification" if str_field(input, "notification_type") == Some("agent_completed") => {
            drop_first(&mut list, "shell")
        }
        _ => {}
    }
    if list.len() > MAX_BACKGROUND {
        list.drain(..list.len() - MAX_BACKGROUND);
    }
    if list == before {
        return false;
    }
    if list.is_empty() {
        rec.remove("background");
    } else {
        rec.insert("background".into(), Value::Array(list));
    }
    true
}

fn is_task_notification(input: &Value) -> bool {
    str_field(input, "prompt").is_some_and(|p| p.trim_start().starts_with("<task-notification>"))
}

/// The tasks a `<task-notification>` prompt reports on: the ids naming each
/// one (`<task-id>`, `<tool-use-id>`) and whether it ended. One prompt can
/// carry several notifications. A finished task has a `<status>`
/// (completed, failed, stopped...); a monitor's events and a shell waiting
/// for input come without one, except a monitor that expired.
fn task_notifications(input: &Value) -> Vec<(Vec<String>, bool)> {
    if !is_task_notification(input) {
        return Vec::new();
    }
    let prompt = str_field(input, "prompt").unwrap_or("");
    prompt
        .split("<task-notification>")
        .skip(1)
        .map(|block| {
            let block = block.split("</task-notification>").next().unwrap_or(block);
            let ids = ["task-id", "tool-use-id"]
                .iter()
                .filter_map(|tag| last_element(block, tag))
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect();
            let status = last_element(block, "status").map(str::trim);
            let ended = match status {
                Some(s) => !matches!(s, "running" | "in_progress" | "pending"),
                None => last_element(block, "event")
                    .is_some_and(|e| e.trim_start().starts_with("[Monitor expired")),
            };
            (ids, ended)
        })
        .collect()
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
    let text = unwrap_envelope(s);
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    clip_to(&joined, PROMPT_MAX_CHARS)
}

/// Sessions driven from a claude.ai project thread receive the person's
/// message wrapped in markup (`<wake>…<message>text</message>…</wake>`, or a
/// `<relay>…<note>text</note>` brief). Keeps only the text a person wrote.
fn unwrap_envelope(s: &str) -> String {
    let trimmed = s.trim_start();
    if !trimmed.starts_with('<') {
        return s.to_string();
    }
    let inner = last_element(trimmed, "message")
        .or_else(|| last_element(trimmed, "note"))
        .unwrap_or(trimmed);
    unescape(&strip_tags(inner))
}

/// Content of the last `<name …>…</name>` element in `s`.
fn last_element<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let close = format!("</{name}>");
    let end = s.rfind(&close)?;
    let open = s[..end].rfind(&format!("<{name}"))?;
    let body = open + s[open..end].find('>')? + 1;
    Some(&s[body..end])
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
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
    fn background_work_keeps_the_session_busy() {
        let bg = |tool: &str, desc: &str| {
            ev(
                "PreToolUse",
                json!({ "tool_name": tool, "tool_use_id": desc,
                        "tool_input": { "description": desc, "run_in_background": true } }),
            )
        };
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 1);
        let r = step(r, bg("Bash", "Start dev server"), 2);
        let r = step(r, bg("Agent", "Explore the API"), 3);
        // A foreground call doesn't count.
        let r = step(
            r,
            ev(
                "PreToolUse",
                json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }),
            ),
            4,
        );
        let r = step(r, ev("Stop", json!({})), 5).unwrap();
        assert_eq!(r["state"], "background");
        assert_eq!(r["detail"], "Start dev server");
        assert_eq!(r["background"].as_array().unwrap().len(), 2);

        // The subagent reports back, then the shell.
        let r = step(Some(r), ev("SubagentStop", json!({ "agent_id": "a1" })), 6);
        let r = step(r, ev("Stop", json!({})), 7).unwrap();
        assert_eq!(r["state"], "background");
        let done = ev(
            "Notification",
            json!({ "notification_type": "agent_completed" }),
        );
        let r = step(Some(r), done, 8);
        let r = step(r, ev("Stop", json!({})), 9).unwrap();
        assert_eq!(r["state"], "done");
        assert!(r.get("background").is_none());
    }

    #[test]
    fn stopping_a_shell_forgets_it() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 1);
        let r = step(
            r,
            ev(
                "PreToolUse",
                json!({ "tool_name": "Bash",
                "tool_input": { "command": "npm run dev", "run_in_background": true } }),
            ),
            2,
        );
        let r = step(
            r,
            ev(
                "PreToolUse",
                json!({ "tool_name": "KillShell", "tool_input": {} }),
            ),
            3,
        );
        let r = step(r, ev("Stop", json!({})), 4).unwrap();
        assert_eq!(r["state"], "done");
    }

    fn notification(task: &str, tool_use: &str, tail: &str) -> Value {
        let prompt = format!(
            "<task-notification>\n<task-id>{task}</task-id>\n<tool-use-id>{tool_use}</tool-use-id>\n\
             <output-file>C:\\tmp\\{task}.output</output-file>\n{tail}\n</task-notification>"
        );
        ev("UserPromptSubmit", json!({ "prompt": prompt }))
    }

    fn started(tool: &str, id: &str, input: Value, task: &str) -> [Value; 2] {
        let key = if tool == "Monitor" {
            "taskId"
        } else {
            "backgroundTaskId"
        };
        [
            ev(
                "PreToolUse",
                json!({ "tool_name": tool, "tool_use_id": id, "tool_input": input }),
            ),
            ev(
                "PostToolUse",
                json!({ "tool_name": tool, "tool_use_id": id, "tool_input": input,
                        "tool_response": { key: task } }),
            ),
        ]
    }

    fn pending(r: &Value) -> Vec<&str> {
        r["background"]
            .as_array()
            .map(|l| l.iter().filter_map(|b| b["detail"].as_str()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn task_notifications_end_background_work() {
        let mut r = step(
            None,
            ev("UserPromptSubmit", json!({ "prompt": "Prueba Arnidol" })),
            1,
        );
        let shell = json!({ "command": "npx ng serve", "description": "Dev server", "run_in_background": true });
        let monitor = json!({ "command": "until grep ...", "description": "Wait for build" });
        for e in started("Bash", "toolu_A", shell, "bsh1")
            .into_iter()
            .chain(started("Monitor", "toolu_B", monitor, "bmo1"))
        {
            r = step(r, e, 2);
        }
        let r = r.unwrap();
        assert_eq!(pending(&r), ["Dev server", "Wait for build"]);
        // While Claude still works, the state is the work, the list stays.
        assert_eq!(r["state"], "thinking");

        let r = step(Some(r), ev("Stop", json!({})), 3).unwrap();
        assert_eq!(r["state"], "background");

        // A monitor event is news, not the end.
        let event = notification(
            "bmo1",
            "toolu_B",
            "<summary>Monitor event</summary>\n<event>Compiled</event>",
        );
        let r = step(Some(r), event, 4).unwrap();
        assert_eq!(pending(&r).len(), 2);
        // The notification doesn't replace the person's prompt or steps.
        assert_eq!(r["prompt"], "Prueba Arnidol");
        assert_eq!(r["steps"].as_array().unwrap().len(), 2);

        let ended = notification(
            "bmo1",
            "toolu_B",
            "<status>completed</status>\n<summary>stream ended</summary>",
        );
        let r = step(Some(r), ended, 5);
        let r = step(r, ev("Stop", json!({})), 6).unwrap();
        assert_eq!(
            (r["state"].as_str(), pending(&r)),
            (Some("background"), vec!["Dev server"])
        );

        let killed = notification("bsh1", "toolu_A", "<status>failed</status>");
        let r = step(Some(r), killed, 7);
        let r = step(r, ev("Stop", json!({})), 8).unwrap();
        assert_eq!(r["state"], "done");
        assert!(r.get("background").is_none());
    }

    #[test]
    fn expired_monitors_and_stopped_tasks_leave() {
        let mut r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 1);
        let a = json!({ "command": "a", "description": "Server A", "run_in_background": true });
        let b = json!({ "command": "b", "description": "Server B", "run_in_background": true });
        let m = json!({ "command": "m", "description": "Watch" });
        for e in started("Bash", "tA", a, "ta")
            .into_iter()
            .chain(started("Bash", "tB", b, "tb"))
            .chain(started("Monitor", "tM", m, "tm"))
        {
            r = step(r, e, 2);
        }
        // TaskStop names the task: B goes, not the first shell.
        let stop = ev(
            "PreToolUse",
            json!({ "tool_name": "TaskStop", "tool_input": { "task_id": "tb" } }),
        );
        let r = step(r, stop, 3).unwrap();
        assert_eq!(pending(&r), ["Server A", "Watch"]);

        let expired = notification(
            "tm",
            "",
            "<summary>Monitor event</summary>\n<event>[Monitor expired after 5m with no events delivered.]</event>",
        );
        let r = step(Some(r), expired, 4).unwrap();
        assert_eq!(pending(&r), ["Server A"]);
    }

    #[test]
    fn commands_sent_to_the_background_later_count() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 1);
        // Started in the foreground; Claude Code moved it to the background.
        let post = ev(
            "PostToolUse",
            json!({ "tool_name": "Bash", "tool_use_id": "t1",
                    "tool_input": { "command": "npm test", "description": "Run tests" },
                    "tool_response": { "stdout": "", "backgroundTaskId": "b1" } }),
        );
        let r = step(r, post, 2);
        let r = step(r, ev("Stop", json!({})), 3).unwrap();
        assert_eq!(
            (r["state"].as_str(), r["detail"].as_str()),
            (Some("background"), Some("Run tests"))
        );

        // A foreground subagent finishing leaves a background one alone.
        let agent = json!({ "description": "Explore", "run_in_background": true });
        let mut r = Some(r);
        for e in started("Agent", "t2", agent, "ag1") {
            r = step(r, e, 4);
        }
        let r = step(r, ev("SubagentStop", json!({ "agent_id": "other" })), 5).unwrap();
        assert_eq!(pending(&r), ["Run tests", "Explore"]);
        let r = step(Some(r), ev("SubagentStop", json!({ "agent_id": "ag1" })), 6).unwrap();
        assert_eq!(pending(&r), ["Run tests"]);
    }

    #[test]
    fn project_stays_on_the_starting_folder() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 1).unwrap();
        let mut bash = ev(
            "PreToolUse",
            json!({ "tool_name": "Bash", "tool_input": {} }),
        );
        bash["cwd"] = json!("/w/Lulo/crates/src");
        let r = step(Some(r), bash, 2).unwrap();
        assert_eq!(
            (r["project"].as_str(), r["cwd"].as_str()),
            (Some("Lulo"), Some("/w/Lulo"))
        );

        let mut start = ev("SessionStart", json!({}));
        start["cwd"] = json!("/w/Other");
        let r = step(Some(r), start, 3).unwrap();
        assert_eq!(r["project"], "Other");
    }

    #[test]
    fn project_thread_prompts_keep_only_the_message() {
        let prompt = |p: &str| {
            let r = step(None, ev("UserPromptSubmit", json!({ "prompt": p })), 1).unwrap();
            r["prompt"].as_str().unwrap().to_string()
        };
        let wake = r#"<wake reason="mention"> <project id="p"> <thread ts="t">
            <message trigger="true" from="human" id="m1">Aplica los cambios &#34;ya&#34;</message>
            </thread> </project> </wake>"#;
        assert_eq!(prompt(wake), "Aplica los cambios \"ya\"");

        let relay = r#"<relay from="coordinator" session="s"> The note below was written by
            the coordinator. <note> Diagnostic audit of Lulo&#39;s hooks </note> </relay>"#;
        assert_eq!(prompt(relay), "Diagnostic audit of Lulo's hooks");

        assert_eq!(prompt("usa <b>negrita</b>"), "usa <b>negrita</b>");
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

    #[test]
    fn since_marks_when_the_state_began() {
        let r = step(None, ev("SessionStart", json!({})), 1).unwrap();
        assert_eq!(r["since"], 1);
        let r = step(Some(r), ev("UserPromptSubmit", json!({ "prompt": "x" })), 5).unwrap();
        assert_eq!(
            (r["state"].as_str(), r["since"].as_u64()),
            (Some("thinking"), Some(5))
        );
        // Another event with the same state keeps the start.
        let r = step(
            Some(r),
            ev("PostToolUse", json!({ "tool_name": "Read" })),
            9,
        )
        .unwrap();
        assert_eq!(r["since"], 5);
        let r = step(
            Some(r),
            ev("PermissionRequest", json!({ "tool_name": "Bash" })),
            12,
        )
        .unwrap();
        assert_eq!(
            (r["state"].as_str(), r["since"].as_u64()),
            (Some("waiting"), Some(12))
        );
        // Files written by an older hook have no `since`: it starts now.
        let mut old = r.clone();
        old.as_object_mut().unwrap().remove("since");
        let r = step(
            Some(old),
            ev("PermissionRequest", json!({ "tool_name": "Bash" })),
            20,
        )
        .unwrap();
        assert_eq!(r["since"], 20);
    }
}
