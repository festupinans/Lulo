//! Reads the status files `lulo-hook` writes, one per Claude Code session.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: String,
    pub project: String,
    pub state: String,
    pub detail: Option<String>,
    /// Unix seconds of the last hook event.
    pub ts: u64,
    /// The last prompt, clipped by the hook.
    pub prompt: Option<String>,
    /// Claude's task list, when it keeps one.
    pub tasks: Vec<Task>,
    /// Shells and subagents still running in the background.
    pub background: usize,
    /// Unix seconds when the current state began (newer hooks only).
    pub since: Option<u64>,
    /// Unix seconds when the last prompt was sent.
    pub started: Option<u64>,
    /// The Claude Code process, to bring its window to the front.
    pub claude_pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub text: String,
    /// "pending", "in_progress" or "completed".
    pub status: String,
}

impl Session {
    pub fn parse(text: &str) -> Option<Session> {
        let v: Value = serde_json::from_str(text).ok()?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Some(Session {
            id: s("session_id")?,
            project: s("project")
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| "?".into()),
            state: s("state")?,
            detail: s("detail").filter(|d| !d.is_empty()),
            ts: v.get("ts").and_then(Value::as_u64).unwrap_or(0),
            prompt: s("prompt").filter(|p| !p.is_empty()),
            tasks: array(&v, "tasks")
                .filter_map(|t| {
                    Some(Task {
                        text: t.get("text")?.as_str()?.to_string(),
                        status: t
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or("pending")
                            .to_string(),
                    })
                })
                .collect(),
            background: array(&v, "background").count(),
            since: v.get("since").and_then(Value::as_u64),
            started: v.get("started").and_then(Value::as_u64),
            claude_pid: v
                .get("claude_pid")
                .and_then(Value::as_u64)
                .and_then(|p| u32::try_from(p).ok()),
        })
    }

    /// (completed, total) tasks.
    pub fn task_counts(&self) -> (usize, usize) {
        let done = self
            .tasks
            .iter()
            .filter(|t| t.status == "completed")
            .count();
        (done, self.tasks.len())
    }

    /// The state to show: any state except "waiting" turns "inactive" after
    /// `inactive_secs` without hook events. Waiting stays visible because it
    /// needs the user. A session mid-turn sends nothing while a long command
    /// or a long thought runs, so busy states get at least `BUSY_GRACE_SECS`.
    pub fn shown_state(&self, now: u64, inactive_secs: u64) -> &str {
        let limit = if BUSY_STATES.contains(&self.state.as_str()) {
            inactive_secs.max(BUSY_GRACE_SECS)
        } else {
            inactive_secs
        };
        if self.state != "waiting" && now.saturating_sub(self.ts) >= limit {
            "inactive"
        } else {
            &self.state
        }
    }
}

impl Session {
    /// Seconds the session has been in its shown state. Busy states count
    /// from the prompt, since thinking and tools alternate all through a
    /// turn; "inactive" counts from the last event.
    pub fn elapsed(&self, shown: &str, now: u64) -> Option<u64> {
        let from = match shown {
            "inactive" => Some(self.ts),
            s if BUSY_STATES.contains(&s) && s != "background" => self.started.or(self.since),
            _ => self.since,
        }?;
        (from > 0).then(|| now.saturating_sub(from))
    }
}

/// Short Spanish duration: "ahora", "4 min", "1 h 12".
pub fn duration(secs: u64) -> String {
    match secs {
        0..60 => "ahora".into(),
        60..3600 => format!("{} min", secs / 60),
        _ if secs % 3600 < 60 => format!("{} h", secs / 3600),
        _ => format!("{} h {:02}", secs / 3600, secs % 3600 / 60),
    }
}

/// States of a session in the middle of a turn.
const BUSY_STATES: &[&str] = &[
    "thinking",
    "editing",
    "bash",
    "reading",
    "subagent",
    "tool",
    "background",
];
/// Longer than Claude Code's 10-minute cap on a foreground Bash command.
const BUSY_GRACE_SECS: u64 = 20 * 60;

fn array<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).into_iter().flatten()
}

/// Spanish label for a state.
pub fn label(state: &str) -> &'static str {
    match state {
        "ready" => "Lista",
        "thinking" => "Pensando",
        "editing" => "Editando",
        "bash" => "Bash",
        "reading" => "Leyendo",
        "subagent" => "Subagente",
        "tool" => "Herramienta",
        "waiting" => "Esperando",
        "background" => "En segundo plano",
        "done" => "Terminó",
        "error" => "Error",
        "inactive" => "Inactiva",
        _ => "Desconocido",
    }
}

/// Same folder `lulo-hook` writes to (kept in sync with its `status_dir`).
pub fn status_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("LULO_STATUS_DIR") {
        return Some(dir);
    }
    if cfg!(windows) {
        return env("LOCALAPPDATA").map(|d| d.join("claude-status"));
    }
    env("XDG_STATE_HOME")
        .or_else(|| env("HOME").map(|h| h.join(".local").join("state")))
        .map(|d| d.join("claude-status"))
}

/// Every readable session in `dir`, most recently active first. Unreadable
/// or half-written files are skipped; the next change event retries them.
///
/// Sessions silent for `forget_secs` are deleted: a terminal closed without
/// `/exit` never sends SessionEnd, so its file would otherwise stay forever.
pub fn load(dir: &Path, now: u64, forget_secs: u64) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<Session> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        // `_usage.json` and any other non-session file the hook keeps here.
        .filter(|p| {
            !p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('_'))
        })
        .filter_map(|p| {
            let session = Session::parse(&fs::read_to_string(&p).ok()?)?;
            if now.saturating_sub(session.ts) >= forget_secs {
                let _ = fs::remove_file(&p);
                return None;
            }
            Some(session)
        })
        .collect();
    sessions.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.project.cmp(&b.project)));
    sessions
}

/// Deletes a session's file so it leaves the list. If the session gets
/// another hook event, the hook writes it again and it comes back.
pub fn forget(dir: &Path, id: &str) {
    // Same rule as the hook: a crafted id can't reach outside the folder.
    let valid = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if valid {
        let _ = fs::remove_file(dir.join(format!("{id}.json")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hook_output() {
        let s = Session::parse(
            r#"{"session_id":"abc","project":"Lulo","cwd":"C:\\dev\\Lulo","state":"editing","detail":"main.rs","event":"PreToolUse","ts":100}"#,
        )
        .unwrap();
        assert_eq!(s.project, "Lulo");
        assert_eq!(label(&s.state), "Editando");
        assert_eq!(s.detail.as_deref(), Some("main.rs"));
        assert_eq!(s.ts, 100);

        let s = Session::parse(r#"{"session_id":"x","project":"","state":"done","detail":null}"#)
            .unwrap();
        assert_eq!((s.project.as_str(), s.detail, s.ts), ("?", None, 0));

        assert!(Session::parse("{").is_none());
        assert!(Session::parse(r#"{"state":"done"}"#).is_none());
    }

    #[test]
    fn parses_progress() {
        let s = Session::parse(
            r#"{"session_id":"a","state":"editing","ts":9,"prompt":"Arregla el login","started":5,
                "steps":[{"state":"reading","detail":"a.rs","ts":6},{"state":"editing","detail":null,"ts":8},{"bad":1}],
                "tasks":[{"id":"1","text":"Leer","status":"completed"},{"id":"2","text":"Arreglar","status":"in_progress"},{"text":"Probar"}]}"#,
        )
        .unwrap();
        assert_eq!(s.prompt.as_deref(), Some("Arregla el login"));
        assert_eq!(s.tasks[2].status, "pending");
        assert_eq!(s.task_counts(), (1, 3));
    }

    #[test]
    fn elapsed_time_per_state() {
        let mut s = Session::parse(
            r#"{"session_id":"a","state":"waiting","ts":90,"since":80,"started":20}"#,
        )
        .unwrap();
        assert_eq!(s.elapsed("waiting", 100), Some(20));
        assert_eq!(s.elapsed("bash", 100), Some(80));
        assert_eq!(s.elapsed("inactive", 100), Some(10));
        s.since = None;
        assert_eq!(s.elapsed("done", 100), None);
        assert_eq!(duration(59), "ahora");
        assert_eq!(duration(4 * 60 + 5), "4 min");
        assert_eq!(duration(3600 + 12 * 60), "1 h 12");
        assert_eq!(duration(2 * 3600 + 30), "2 h");
    }

    #[test]
    fn busy_sessions_get_a_longer_grace() {
        let mut s = Session::parse(r#"{"session_id":"a","state":"bash","ts":0}"#).unwrap();
        assert_eq!(s.shown_state(600, 300), "bash");
        assert_eq!(s.shown_state(BUSY_GRACE_SECS, 300), "inactive");
        s.state = "done".into();
        assert_eq!(s.shown_state(300, 300), "inactive");
        s.state = "ready".into();
        assert_eq!(s.shown_state(300, 300), "inactive");
    }

    #[test]
    fn loads_json_files_newest_first() {
        let dir = std::env::temp_dir().join(format!("lulo-widget-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("a.json"),
            r#"{"session_id":"a","project":"Old","state":"done","ts":1}"#,
        )
        .unwrap();
        fs::write(
            dir.join("b.json"),
            r#"{"session_id":"b","project":"New","state":"bash","ts":9}"#,
        )
        .unwrap();
        fs::write(
            dir.join("c.json.123.tmp"),
            r#"{"session_id":"c","state":"bash","ts":5}"#,
        )
        .unwrap();
        fs::write(dir.join("d.json"), "half-writ").unwrap();
        fs::write(
            dir.join("_usage.json"),
            r#"{"session_id":"u","state":"done","ts":1}"#,
        )
        .unwrap();

        let names: Vec<_> = load(&dir, 10, 100).into_iter().map(|s| s.project).collect();
        assert_eq!(names, ["New", "Old"]);
        assert!(dir.join("a.json").exists());

        // At now=101, "Old" (ts 1) is 100 s silent: forgotten and deleted.
        let names: Vec<_> = load(&dir, 101, 100)
            .into_iter()
            .map(|s| s.project)
            .collect();
        assert_eq!(names, ["New"]);
        assert!(!dir.join("a.json").exists());
        assert!(dir.join("_usage.json").exists());
        fs::remove_dir_all(&dir).unwrap();

        assert!(load(Path::new("/definitely/not/here"), 0, 1).is_empty());
    }

    #[test]
    fn silent_sessions_turn_inactive_except_waiting() {
        let mut s = Session::parse(r#"{"session_id":"a","state":"done","ts":1000}"#).unwrap();
        assert_eq!(s.shown_state(1299, 300), "done");
        assert_eq!(s.shown_state(1300, 300), "inactive");
        s.state = "thinking".into();
        assert_eq!(s.shown_state(5000, 300), "inactive");
        s.state = "waiting".into();
        assert_eq!(s.shown_state(5000, 300), "waiting");
        assert_eq!(label("inactive"), "Inactiva");
    }

    #[test]
    fn forgets_only_the_session_asked() {
        let dir = std::env::temp_dir().join(format!("lulo-forget-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["a.json", "b.json", "sub/c.json"] {
            fs::write(dir.join(f), "{}").unwrap();
        }
        forget(&dir, "a");
        forget(&dir, "../b");
        forget(&dir, "sub/c");
        assert!(!dir.join("a.json").exists());
        assert!(dir.join("b.json").exists());
        assert!(dir.join("sub/c.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
