//! Maps a Claude Code hook event to the session state the widget shows.
//!
//! Pure functions only, so the whole mapping is unit-testable without
//! touching the filesystem.

use serde_json::Value;

/// What the hook should do with the session's status file.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Write the file with this state and optional detail (file name, command...).
    Write {
        state: State,
        detail: Option<String>,
    },
    /// Keep the current state but refresh the timestamp (activity inside a subagent).
    Touch,
    /// Delete the status file (the session ended).
    Remove,
    /// Nothing to record for this event.
    Ignore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Session just started, no prompt yet.
    Ready,
    Thinking,
    Editing,
    Bash,
    Reading,
    Subagent,
    /// Any other tool (MCP tools, TodoWrite, ...).
    Tool,
    Waiting,
    Done,
    Error,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Ready => "ready",
            State::Thinking => "thinking",
            State::Editing => "editing",
            State::Bash => "bash",
            State::Reading => "reading",
            State::Subagent => "subagent",
            State::Tool => "tool",
            State::Waiting => "waiting",
            State::Done => "done",
            State::Error => "error",
        }
    }
}

const DETAIL_MAX_CHARS: usize = 80;

/// Notification types that mean the session is blocked on the user.
const WAITING_NOTIFICATIONS: &[&str] = &[
    "permission_prompt",
    "idle_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
    "agent_needs_input",
];

pub fn classify(input: &Value) -> Action {
    let event = str_field(input, "hook_event_name").unwrap_or("");
    let tool = str_field(input, "tool_name").unwrap_or("");
    // Events fired from inside a subagent carry agent_id. They must not
    // overwrite the parent's "subagent" state, only show that it's alive.
    let in_subagent = input.get("agent_id").is_some_and(|v| !v.is_null());

    match event {
        "SessionStart" => write(State::Ready, None),
        "UserPromptSubmit" => write(State::Thinking, None),
        "PreToolUse" if in_subagent => Action::Touch,
        "PreToolUse" => {
            let state = tool_state(tool);
            write(state, tool_detail(tool, input.get("tool_input")))
        }
        "PostToolUse" | "PostToolUseFailure" | "SubagentStop" if in_subagent => Action::Touch,
        "PostToolUse" | "PostToolUseFailure" | "SubagentStop" => write(State::Thinking, None),
        // A permission prompt blocks the whole session even when a subagent asked.
        "PermissionRequest" => write(
            State::Waiting,
            Some(tool.to_string()).filter(|t| !t.is_empty()),
        ),
        "Notification" => {
            let kind = str_field(input, "notification_type").unwrap_or("");
            if WAITING_NOTIFICATIONS.contains(&kind) {
                write(State::Waiting, str_field(input, "message").map(truncate))
            } else {
                Action::Ignore
            }
        }
        "Stop" => write(State::Done, None),
        "StopFailure" => write(State::Error, None),
        "SessionEnd" => Action::Remove,
        _ => Action::Ignore,
    }
}

pub fn tool_state(tool: &str) -> State {
    match tool {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => State::Editing,
        "Bash" | "PowerShell" | "BashOutput" => State::Bash,
        "Read" | "Grep" | "Glob" | "LS" | "WebFetch" | "WebSearch" => State::Reading,
        // "Task" is the subagent tool's former name.
        "Agent" | "Task" => State::Subagent,
        _ => State::Tool,
    }
}

/// A short human hint for the widget: the file being edited, the command, etc.
fn tool_detail(tool: &str, tool_input: Option<&Value>) -> Option<String> {
    let input = tool_input?;
    let field = |k: &str| str_field(input, k);
    let detail = match tool_state(tool) {
        State::Editing | State::Reading => field("file_path")
            .or_else(|| field("notebook_path"))
            .map(file_name)
            .or_else(|| field("pattern"))
            .or_else(|| field("url"))
            .or_else(|| field("query")),
        State::Bash => field("description").or_else(|| field("command")),
        State::Subagent => field("description").or_else(|| field("subagent_type")),
        _ => Some(tool),
    }?;
    Some(truncate(detail))
}

fn write(state: State, detail: Option<String>) -> Action {
    Action::Write { state, detail }
}

fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}

/// Last path component, accepting both `/` and `\` so Windows paths work
/// whichever platform the tests run on.
pub fn file_name(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed)
}

fn truncate(s: &str) -> String {
    let one_line = s.lines().next().unwrap_or("").trim();
    if one_line.chars().count() <= DETAIL_MAX_CHARS {
        return one_line.to_string();
    }
    let mut out: String = one_line.chars().take(DETAIL_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn state_of(v: Value) -> Option<State> {
        match classify(&v) {
            Action::Write { state, .. } => Some(state),
            _ => None,
        }
    }

    fn pre(tool: &str, input: Value) -> Value {
        json!({ "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input })
    }

    #[test]
    fn prompt_and_tool_results_mean_thinking() {
        for ev in [
            "UserPromptSubmit",
            "PostToolUse",
            "PostToolUseFailure",
            "SubagentStop",
        ] {
            assert_eq!(
                state_of(json!({ "hook_event_name": ev })),
                Some(State::Thinking),
                "{ev}"
            );
        }
    }

    #[test]
    fn pre_tool_use_maps_by_tool() {
        let cases = [
            ("Edit", State::Editing),
            ("Write", State::Editing),
            ("MultiEdit", State::Editing),
            ("Bash", State::Bash),
            ("PowerShell", State::Bash),
            ("Read", State::Reading),
            ("Grep", State::Reading),
            ("Glob", State::Reading),
            ("Agent", State::Subagent),
            ("Task", State::Subagent),
            ("mcp__github__get_me", State::Tool),
        ];
        for (tool, want) in cases {
            assert_eq!(state_of(pre(tool, json!({}))), Some(want), "{tool}");
        }
    }

    #[test]
    fn details_are_short_and_useful() {
        let edit = classify(&pre(
            "Edit",
            json!({ "file_path": "C:\\dev\\Lulo\\src\\main.rs" }),
        ));
        assert_eq!(edit, write(State::Editing, Some("main.rs".into())));

        let bash = classify(&pre("Bash", json!({ "command": "cargo test\necho done" })));
        assert_eq!(bash, write(State::Bash, Some("cargo test".into())));

        let long = "x".repeat(200);
        let Action::Write {
            detail: Some(d), ..
        } = classify(&pre("Bash", json!({ "command": long })))
        else {
            panic!("expected a detail");
        };
        assert_eq!(d.chars().count(), DETAIL_MAX_CHARS);
        assert!(d.ends_with('…'));

        let grep = classify(&pre("Grep", json!({ "pattern": "fn main" })));
        assert_eq!(grep, write(State::Reading, Some("fn main".into())));

        let agent = classify(&pre("Agent", json!({ "description": "Explore repo" })));
        assert_eq!(agent, write(State::Subagent, Some("Explore repo".into())));
    }

    #[test]
    fn subagent_activity_only_touches() {
        let mut v = pre("Bash", json!({ "command": "ls" }));
        v["agent_id"] = json!("sub-1");
        assert_eq!(classify(&v), Action::Touch);

        let post =
            json!({ "hook_event_name": "PostToolUse", "tool_name": "Read", "agent_id": "sub-1" });
        assert_eq!(classify(&post), Action::Touch);
    }

    #[test]
    fn waiting_states() {
        let perm = json!({ "hook_event_name": "PermissionRequest", "tool_name": "Bash", "agent_id": "sub-1" });
        assert_eq!(classify(&perm), write(State::Waiting, Some("Bash".into())));

        for kind in ["permission_prompt", "idle_prompt", "elicitation_dialog"] {
            let n = json!({ "hook_event_name": "Notification", "notification_type": kind, "message": "Needs you" });
            assert_eq!(
                classify(&n),
                write(State::Waiting, Some("Needs you".into())),
                "{kind}"
            );
        }

        let other =
            json!({ "hook_event_name": "Notification", "notification_type": "auth_success" });
        assert_eq!(classify(&other), Action::Ignore);
    }

    #[test]
    fn lifecycle_events() {
        assert_eq!(
            state_of(json!({ "hook_event_name": "SessionStart", "source": "startup" })),
            Some(State::Ready)
        );
        assert_eq!(
            state_of(json!({ "hook_event_name": "Stop" })),
            Some(State::Done)
        );
        assert_eq!(
            state_of(json!({ "hook_event_name": "StopFailure" })),
            Some(State::Error)
        );
        assert_eq!(
            classify(&json!({ "hook_event_name": "SessionEnd", "reason": "other" })),
            Action::Remove
        );
        assert_eq!(
            classify(&json!({ "hook_event_name": "PreCompact" })),
            Action::Ignore
        );
        assert_eq!(classify(&json!({})), Action::Ignore);
    }

    #[test]
    fn file_name_handles_both_separators() {
        assert_eq!(file_name("C:\\a\\b\\Lulo"), "Lulo");
        assert_eq!(file_name("/home/me/Lulo/"), "Lulo");
        assert_eq!(file_name("C:\\"), "C:");
        assert_eq!(file_name("Lulo"), "Lulo");
    }
}
