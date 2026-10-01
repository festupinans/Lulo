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

            // A background task reporting back wakes Claude up, but it is
            // still working on the person's last message.
            if event == "UserPromptSubmit" && task_notification(input).is_none() {
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

/// Tracks what keeps running beside the main agent: shells started with
/// `run_in_background`, Monitor watches (always in the background) and
/// subagents (SubagentStart to SubagentStop). Claude Code tells Claude that
/// a background shell ended through a `<task-notification>` prompt, which
/// names it, so that entry is dropped. Returns whether the list changed.
fn update_background(rec: &mut Map<String, Value>, input: &Value) -> bool {
    let event = str_field(input, "hook_event_name").unwrap_or("");
    let tool = str_field(input, "tool_name").unwrap_or("");
    let tool_input = input.get("tool_input").unwrap_or(&Value::Null);
    let in_subagent = input.get("agent_id").is_some_and(|v| !v.is_null());
    let mut list = background(rec);
    let before = list.clone();
    let drop_first = |list: &mut Vec<Value>, kind: &str| {
        if let Some(i) = list
            .iter()
            .position(|b| b.get("kind").and_then(Value::as_str) == Some(kind))
        {
            list.remove(i);
        }
    };
    // An entry is known by the tool call that started it, by the task id
    // Claude Code gave it, or by the subagent's id.
    let drop_id = |list: &mut Vec<Value>, id: &str| {
        let len = list.len();
        list.retain(|b| str_field(b, "id") != Some(id) && str_field(b, "task") != Some(id));
        list.len() != len
    };

    match event {
        "PreToolUse" if !in_subagent && starts_background(tool, tool_input) => {
            let detail = str_field(tool_input, "description")
                .or_else(|| str_field(tool_input, "command"))
                .map(clip);
            list.push(
                json!({ "id": str_field(input, "tool_use_id"), "kind": "shell", "detail": detail }),
            );
        }
        // The reply to a background call carries the id later messages use.
        "PostToolUse" if !in_subagent => {
            let call = str_field(input, "tool_use_id");
            if let (Some(call), Some(task)) = (call, background_task_id(input)) {
                for b in list.iter_mut().filter(|b| str_field(b, "id") == Some(call)) {
                    b["task"] = json!(task);
                }
            }
        }
        "PreToolUse" if STOP_TOOLS.contains(&tool) => {
            let target = ["task_id", "shell_id", "bash_id"]
                .iter()
                .find_map(|k| str_field(tool_input, k));
            if !target.is_some_and(|id| drop_id(&mut list, id)) {
                drop_first(&mut list, "shell");
            }
        }
        "SubagentStart" => {
            let detail = str_field(input, "agent_type").map(clip);
            list.push(json!({
                "id": str_field(input, "agent_id"), "kind": "agent", "detail": detail
            }));
        }
        "SubagentStop" => {
            let id = str_field(input, "agent_id");
            if !id.is_some_and(|id| drop_id(&mut list, id)) {
                drop_first(&mut list, "agent");
            }
        }
        "UserPromptSubmit" => {
            if let Some(text) = task_notification(input) {
                for tag in ["tool-use-id", "task-id"] {
                    for id in elements(text, tag) {
                        drop_id(&mut list, id.trim());
                    }
                }
            }
        }
        // Subagents already leave through SubagentStop.
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

/// A shell sent to the background, or a Monitor, which always runs there.
fn starts_background(tool: &str, tool_input: &Value) -> bool {
    let flagged = tool_input.get("run_in_background").and_then(Value::as_bool) == Some(true);
    tool == "Monitor" || (flagged && state::tool_state(tool) == state::State::Bash)
}

/// The id Claude Code gave a background shell, from the tool's reply.
fn background_task_id(input: &Value) -> Option<String> {
    let response = input.get("tool_response")?;
    ["backgroundTaskId", "task_id", "taskId", "shell_id"]
        .iter()
        .find_map(|k| str_or_num(response, k))
}

/// The prompt, when it is Claude Code reporting that background work ended
/// rather than something the person typed.
fn task_notification(input: &Value) -> Option<&str> {
    let prompt = str_field(input, "prompt").or_else(|| str_field(input, "user_prompt"))?;
    prompt
        .trim_start()
        .starts_with("<task-notification>")
        .then_some(prompt)
}

/// Contents of every `<name>…</name>` element in `s`.
fn elements<'a>(s: &'a str, name: &str) -> Vec<&'a str> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find(&open) {
        let body = &rest[start + open.len()..];
        let Some(end) = body.find(&close) else { break };
        out.push(&body[..end]);
        rest = &body[end + close.len()..];
    }
    out
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
        let agent = json!({ "agent_id": "a1", "agent_type": "Explore" });
        let r = step(r, ev("SubagentStart", agent.clone()), 3);
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
        let r = step(Some(r), ev("SubagentStop", agent), 6);
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
    fn running_work_is_counted_while_claude_works() {
        let r = step(None, ev("UserPromptSubmit", json!({ "prompt": "x" })), 1);
        let monitor = ev(
            "PreToolUse",
            json!({ "tool_name": "Monitor", "tool_use_id": "m1",
                    "tool_input": { "description": "Wait for the build" } }),
        );
        let r = step(r, monitor, 2);
        let r = step(
            r,
            ev(
                "PreToolUse",
                json!({ "tool_name": "Edit", "tool_input": {} }),
            ),
            3,
        )
        .unwrap();
        assert_eq!(r["state"], "editing");
        assert_eq!(r["background"][0]["detail"], "Wait for the build");
    }

    #[test]
    fn a_task_notification_drops_its_task_and_keeps_the_prompt() {
        let r = step(
            None,
            ev(
                "UserPromptSubmit",
                json!({ "prompt": "Arranca el servidor" }),
            ),
            1,
        );
        let start = |id: &str| {
            ev(
                "PreToolUse",
                json!({ "tool_name": "Bash", "tool_use_id": id,
                        "tool_input": { "command": "npm start", "run_in_background": true } }),
            )
        };
        let r = step(r, start("toolu_1"), 2);
        let r = step(r, start("toolu_2"), 3);
        // The second shell's id arrives with its reply.
        let reply = ev(
            "PostToolUse",
            json!({ "tool_name": "Bash", "tool_use_id": "toolu_2",
                    "tool_response": { "backgroundTaskId": "b2" } }),
        );
        let r = step(r, reply, 4);
        let r = step(r, ev("Stop", json!({})), 5);

        let note = "<task-notification>\n<task-id>b2</task-id>\n\
                    <status>completed</status>\n</task-notification>";
        let r = step(r, ev("UserPromptSubmit", json!({ "prompt": note })), 6).unwrap();
        assert_eq!(r["state"], "thinking");
        assert_eq!(r["prompt"], "Arranca el servidor");
        assert_eq!(r["started"], 1);
        let left = r["background"].as_array().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0]["id"], "toolu_1");

        let note = "<task-notification><tool-use-id>toolu_1</tool-use-id>\
                    <status>completed</status></task-notification>";
        let r = step(
            Some(r),
            ev("UserPromptSubmit", json!({ "prompt": note })),
            7,
        );
        let r = step(r, ev("Stop", json!({})), 8).unwrap();
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
