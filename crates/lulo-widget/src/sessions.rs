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
        })
    }

    /// Spanish label for the widget.
    pub fn label(&self) -> &'static str {
        match self.state.as_str() {
            "ready" => "Lista",
            "thinking" => "Pensando",
            "editing" => "Editando",
            "bash" => "Bash",
            "reading" => "Leyendo",
            "subagent" => "Subagente",
            "tool" => "Herramienta",
            "waiting" => "Esperando",
            "done" => "Terminó",
            "error" => "Error",
            _ => "Desconocido",
        }
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
pub fn load(dir: &Path) -> Vec<Session> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut sessions: Vec<Session> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| fs::read_to_string(p).ok())
        .filter_map(|t| Session::parse(&t))
        .collect();
    sessions.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.project.cmp(&b.project)));
    sessions
}

/// "ahora", "5 min", "2 h": coarse on purpose so the window only needs to
/// repaint every few seconds.
pub fn ago(now: u64, ts: u64) -> String {
    let secs = now.saturating_sub(ts);
    match secs {
        0..=59 => "ahora".into(),
        60..=3599 => format!("{} min", secs / 60),
        _ => format!("{} h", secs / 3600),
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
        assert_eq!(s.label(), "Editando");
        assert_eq!(s.detail.as_deref(), Some("main.rs"));
        assert_eq!(s.ts, 100);

        let s = Session::parse(r#"{"session_id":"x","project":"","state":"done","detail":null}"#)
            .unwrap();
        assert_eq!((s.project.as_str(), s.detail, s.ts), ("?", None, 0));

        assert!(Session::parse("{").is_none());
        assert!(Session::parse(r#"{"state":"done"}"#).is_none());
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

        let names: Vec<_> = load(&dir).into_iter().map(|s| s.project).collect();
        assert_eq!(names, ["New", "Old"]);
        fs::remove_dir_all(&dir).unwrap();

        assert!(load(Path::new("/definitely/not/here")).is_empty());
    }

    #[test]
    fn ago_is_coarse() {
        assert_eq!(ago(100, 100), "ahora");
        assert_eq!(ago(100, 200), "ahora");
        assert_eq!(ago(400, 100), "5 min");
        assert_eq!(ago(7300, 100), "2 h");
    }
}
