//! Decides when a session deserves a heads-up: it starts waiting for you,
//! finishes, fails, or a new one opens. The Windows side (toast and sound)
//! lives in `alert`.

use std::collections::HashMap;

use crate::sessions::{self, Session};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Waiting,
    Done,
    Error,
    New,
}

impl Kind {
    /// Most urgent first, to pick the sound when several fire together.
    fn rank(self) -> u8 {
        match self {
            Kind::Waiting => 0,
            Kind::Error => 1,
            Kind::Done => 2,
            Kind::New => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub kind: Kind,
    pub project: String,
    pub detail: Option<String>,
}

impl Event {
    pub fn title(&self) -> String {
        let state = match self.kind {
            Kind::Waiting => sessions::label("waiting"),
            Kind::Done => sessions::label("done"),
            Kind::Error => sessions::label("error"),
            Kind::New => "Nueva sesión",
        };
        format!("{} · {state}", self.project)
    }
}

/// Remembers the last shown state of every session, so only changes alert.
#[derive(Default)]
pub struct Watch {
    seen: Option<HashMap<String, String>>,
}

impl Watch {
    /// The events since the last call. The first call only takes note, so
    /// opening Lulo doesn't replay what already happened.
    pub fn update(&mut self, rows: &[(&Session, &str)]) -> Vec<Event> {
        let now: HashMap<String, String> = rows
            .iter()
            .map(|(s, st)| (s.id.clone(), st.to_string()))
            .collect();
        let Some(seen) = self.seen.replace(now) else {
            return Vec::new();
        };
        let mut events: Vec<Event> = rows
            .iter()
            .filter_map(|(s, st)| {
                let before = seen.get(&s.id).map(String::as_str);
                let kind = kind_of(before, st)?;
                Some(Event {
                    kind,
                    project: s.project.clone(),
                    detail: s.detail.clone(),
                })
            })
            .collect();
        events.sort_by_key(|e| e.kind.rank());
        events
    }
}

fn kind_of(before: Option<&str>, now: &str) -> Option<Kind> {
    if before == Some(now) {
        return None;
    }
    match (before, now) {
        (_, "waiting") => Some(Kind::Waiting),
        (_, "error") => Some(Kind::Error),
        // A session that comes back from "inactive" already finished then.
        (Some("inactive"), "done") => None,
        (_, "done") => Some(Kind::Done),
        (None, "ready") => Some(Kind::New),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str) -> Session {
        Session::parse(&format!(
            r#"{{"session_id":"{id}","project":"P{id}","state":"x"}}"#
        ))
        .unwrap()
    }

    #[test]
    fn alerts_only_on_changes() {
        let (a, b) = (session("a"), session("b"));
        let mut w = Watch::default();
        assert!(w.update(&[(&a, "waiting")]).is_empty());
        assert!(w.update(&[(&a, "waiting")]).is_empty());

        let e = w.update(&[(&a, "thinking"), (&b, "ready")]);
        assert_eq!(e.len(), 1);
        assert_eq!(
            (e[0].kind, e[0].title()),
            (Kind::New, "Pb · Nueva sesión".into())
        );

        let e = w.update(&[(&a, "done"), (&b, "waiting")]);
        let kinds: Vec<_> = e.iter().map(|e| e.kind).collect();
        assert_eq!(kinds, [Kind::Waiting, Kind::Done]);

        assert!(w.update(&[(&a, "inactive"), (&b, "error")])[0].kind == Kind::Error);
        assert!(w.update(&[(&a, "done"), (&b, "error")]).is_empty());
    }

    #[test]
    fn background_work_finishing_counts_as_done() {
        assert_eq!(kind_of(Some("background"), "done"), Some(Kind::Done));
        assert_eq!(kind_of(Some("thinking"), "background"), None);
        assert_eq!(kind_of(Some("done"), "ready"), None);
    }
}
