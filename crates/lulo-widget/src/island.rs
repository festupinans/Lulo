//! Which state the widget acts out: the one that most needs the user.
//! Kept apart from the drawing so it can be tested.

use crate::sessions::Session;

pub fn is_working(state: &str) -> bool {
    matches!(
        state,
        "thinking" | "editing" | "bash" | "reading" | "subagent" | "tool"
    )
}

/// Someone waiting first, then an error, then the latest work, background
/// work, everything finished, and finally quiet. `rows` are most recent
/// first.
pub fn focus(rows: &[(&Session, &str)]) -> &'static str {
    let any = |f: &dyn Fn(&str) -> bool| rows.iter().find(|(_, st)| f(st)).map(|(_, st)| *st);
    if any(&|st| st == "waiting").is_some() {
        return "waiting";
    }
    if any(&|st| st == "error").is_some() {
        return "error";
    }
    if let Some(st) = any(&is_working) {
        return match st {
            "thinking" => "thinking",
            "editing" => "editing",
            "bash" => "bash",
            "reading" => "reading",
            "subagent" => "subagent",
            _ => "tool",
        };
    }
    if any(&|st| st == "background").is_some() {
        return "background";
    }
    if any(&|st| st == "done").is_some() {
        return "done";
    }
    if any(&|st| st == "ready").is_some() {
        return "ready";
    }
    "inactive"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(project: &str) -> Session {
        Session {
            id: project.into(),
            project: project.into(),
            state: String::new(),
            detail: None,
            ts: 0,
            prompt: None,
            tasks: Vec::new(),
            background: 0,
            since: None,
            started: None,
            claude_pid: None,
        }
    }

    #[test]
    fn focus_follows_priority() {
        let a = session("a");
        assert_eq!(
            focus(&[(&a, "editing"), (&a, "subagent"), (&a, "waiting")]),
            "waiting"
        );
        assert_eq!(focus(&[(&a, "editing"), (&a, "error")]), "error");
        assert_eq!(
            focus(&[(&a, "done"), (&a, "bash"), (&a, "editing")]),
            "bash"
        );
        assert_eq!(focus(&[(&a, "done"), (&a, "background")]), "background");
        assert_eq!(focus(&[(&a, "done"), (&a, "inactive")]), "done");
        assert_eq!(focus(&[(&a, "inactive")]), "inactive");
        assert_eq!(focus(&[]), "inactive");
    }
}
