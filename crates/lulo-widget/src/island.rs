//! What the island says: the one-line headline of the collapsed pill and the
//! summary under the chips. Kept apart from the drawing so it can be tested.

use crate::sessions::Session;

/// How a piece of text is colored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Dim,
    /// The color of this state.
    State(&'static str),
}

pub type Line = Vec<(String, Tone)>;

const DETAIL_CHARS: usize = 30;

pub fn is_working(state: &str) -> bool {
    matches!(
        state,
        "thinking" | "editing" | "bash" | "reading" | "subagent" | "tool"
    )
}

/// The collapsed pill's sentence, and the state the octopus acts out. Always
/// about what most needs the user: someone waiting, an error, the latest
/// work, then everything finished or quiet. `rows` are most recent first.
pub fn headline(rows: &[(&Session, &str)]) -> (&'static str, Line) {
    let find = |f: &dyn Fn(&str) -> bool| rows.iter().find(|(_, st)| f(st));
    if let Some((s, _)) = find(&|st| st == "waiting") {
        return (
            "waiting",
            vec![
                (s.project.clone(), Tone::State("waiting")),
                (" te espera".into(), Tone::Normal),
            ],
        );
    }
    if let Some((s, _)) = find(&|st| st == "error") {
        return (
            "error",
            vec![
                (s.project.clone(), Tone::State("error")),
                (" tuvo un error".into(), Tone::Normal),
            ],
        );
    }
    if let Some((s, st)) = find(&|st| is_working(st)) {
        let state = static_state(st);
        let mut line = vec![(
            format!("{} {}", s.project, doing(state, s.detail.as_deref())),
            Tone::Normal,
        )];
        let more = rows.iter().filter(|(_, st)| is_working(st)).count() - 1;
        if more > 0 {
            line.push((format!(" · {more} más"), Tone::Dim));
        }
        return (state, line);
    }
    if rows.iter().any(|(_, st)| matches!(*st, "done" | "ready")) {
        let state = if rows.iter().any(|(_, st)| *st == "done") {
            "done"
        } else {
            "ready"
        };
        return (state, vec![("Todo listo".into(), Tone::Normal)]);
    }
    let text = if rows.is_empty() {
        "Sin sesiones"
    } else {
        "Sin actividad"
    };
    ("inactive", vec![(text.into(), Tone::Dim)])
}

/// "Lulo edita style.rs", "api ejecuta npm test", "docs está pensando".
fn doing(state: &str, detail: Option<&str>) -> String {
    let detail = detail.map(clip).filter(|d| !d.is_empty());
    match (state, detail) {
        ("editing", Some(d)) => format!("edita {d}"),
        ("editing", None) => "está editando".into(),
        ("reading", Some(d)) => format!("lee {d}"),
        ("reading", None) => "está leyendo".into(),
        ("bash", Some(d)) => format!("ejecuta {d}"),
        ("bash", None) => "usa la terminal".into(),
        ("subagent", _) => "usa un subagente".into(),
        ("tool", Some(d)) => format!("usa {d}"),
        ("tool", None) => "usa una herramienta".into(),
        _ => "está pensando".into(),
    }
}

/// "3 trabajando · 1 te espera · 1 terminó", skipping what's zero.
pub fn summary(rows: &[(&Session, &str)]) -> Line {
    let count = |f: &dyn Fn(&str) -> bool| rows.iter().filter(|(_, st)| f(st)).count();
    let parts = [
        (count(&is_working), "trabajando", Tone::Normal),
        (
            count(&|st| st == "waiting"),
            "te espera",
            Tone::State("waiting"),
        ),
        (
            count(&|st| st == "error"),
            "con error",
            Tone::State("error"),
        ),
        (count(&|st| st == "done"), "terminó", Tone::Normal),
        (count(&|st| st == "ready"), "lista", Tone::Normal),
        (count(&|st| st == "inactive"), "inactiva", Tone::Dim),
    ];
    let mut line = Line::new();
    for (n, what, tone) in parts {
        if n == 0 {
            continue;
        }
        if !line.is_empty() {
            line.push((" · ".into(), Tone::Dim));
        }
        let what = match (what, n > 1) {
            ("te espera", true) => "te esperan",
            ("terminó", true) => "terminaron",
            ("lista", true) => "listas",
            ("inactiva", true) => "inactivas",
            _ => what,
        };
        line.push((format!("{n} {what}"), tone));
    }
    if line.is_empty() {
        line.push(("Sin sesiones de Claude Code".into(), Tone::Dim));
    }
    line
}

/// "ahora mismo", "hace 5 min".
pub fn ago(now: u64, ts: u64) -> String {
    match crate::sessions::ago(now, ts).as_str() {
        "ahora" => "ahora mismo".into(),
        other => format!("hace {other}"),
    }
}

/// Which tasks to show as steps: a window around the current one, with how
/// many are hidden before and after it.
pub fn task_window(s: &Session, max: usize) -> (usize, usize, usize) {
    let len = s.tasks.len();
    let current = s
        .tasks
        .iter()
        .position(|t| t.status == "in_progress")
        .or_else(|| s.tasks.iter().position(|t| t.status != "completed"))
        .unwrap_or(len.saturating_sub(1));
    let start = current.saturating_sub(2).min(len.saturating_sub(max));
    let end = (start + max).min(len);
    (start, end, len - end)
}

fn clip(s: &str) -> String {
    let s = s.lines().next().unwrap_or("").trim();
    if s.chars().count() <= DETAIL_CHARS {
        return s.to_string();
    }
    let cut: String = s.chars().take(DETAIL_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

fn static_state(state: &str) -> &'static str {
    match state {
        "thinking" => "thinking",
        "editing" => "editing",
        "bash" => "bash",
        "reading" => "reading",
        "subagent" => "subagent",
        _ => "tool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::Task;

    fn session(project: &str, detail: Option<&str>) -> Session {
        Session {
            id: project.into(),
            project: project.into(),
            state: String::new(),
            detail: detail.map(str::to_string),
            ts: 0,
            prompt: None,
            tasks: Vec::new(),
        }
    }

    fn text(line: &Line) -> String {
        line.iter().map(|(t, _)| t.as_str()).collect()
    }

    #[test]
    fn headline_follows_priority() {
        let lulo = session("Lulo", Some("style.rs"));
        let api = session("api-server", Some("npm test"));
        let docs = session("docs", None);
        let blog = session("blog", None);

        let rows = [(&lulo, "editing"), (&docs, "subagent"), (&api, "waiting")];
        let (state, line) = headline(&rows);
        assert_eq!(
            (state, text(&line).as_str()),
            ("waiting", "api-server te espera")
        );
        assert_eq!(line[0].1, Tone::State("waiting"));

        let rows = [(&lulo, "editing"), (&docs, "error")];
        assert_eq!(text(&headline(&rows).1), "docs tuvo un error");

        let rows = [
            (&lulo, "editing"),
            (&docs, "subagent"),
            (&api, "bash"),
            (&blog, "done"),
        ];
        let (state, line) = headline(&rows);
        assert_eq!(
            (state, text(&line).as_str()),
            ("editing", "Lulo edita style.rs · 2 más")
        );

        let rows = [(&blog, "done"), (&docs, "inactive")];
        assert_eq!(
            headline(&rows),
            ("done", vec![("Todo listo".into(), Tone::Normal)])
        );
        assert_eq!(text(&headline(&[(&docs, "inactive")]).1), "Sin actividad");
        assert_eq!(text(&headline(&[]).1), "Sin sesiones");
    }

    #[test]
    fn long_details_are_clipped() {
        let s = session(
            "api",
            Some("cargo test --workspace --all-features -- --nocapture"),
        );
        let line = headline(&[(&s, "bash")]).1;
        assert_eq!(text(&line), "api ejecuta cargo test --workspace --all-…");
    }

    #[test]
    fn summary_counts_and_skips_zeros() {
        let a = session("a", None);
        let rows = [
            (&a, "editing"),
            (&a, "bash"),
            (&a, "thinking"),
            (&a, "waiting"),
            (&a, "done"),
        ];
        assert_eq!(
            text(&summary(&rows)),
            "3 trabajando · 1 te espera · 1 terminó"
        );
        assert_eq!(text(&summary(&[])), "Sin sesiones de Claude Code");
    }

    #[test]
    fn task_window_centers_on_the_current_task() {
        let mut s = session("a", None);
        for (i, status) in ["completed"; 5]
            .into_iter()
            .chain(["in_progress"])
            .chain(["pending"; 4])
            .enumerate()
        {
            s.tasks.push(Task {
                text: i.to_string(),
                status: status.into(),
            });
        }
        assert_eq!(task_window(&s, 5), (3, 8, 2));
        s.tasks.truncate(3);
        assert_eq!(task_window(&s, 5), (0, 3, 0));
    }
}
