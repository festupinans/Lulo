//! Short reactions of the collapsed octopus: it gets bored and plays, gets
//! annoyed at repeated clicks, follows a nearby mouse with its eyes,
//! celebrates a finished session, startles when one starts waiting and
//! winks now and then. Kept apart from the drawing so the timing can be
//! tested; `octopus::moon` draws whatever `act` returns.

use std::collections::HashMap;

use eframe::egui::Vec2;

use crate::island;
use crate::octopus::Act;

/// Clicks count toward annoyance only this close together (seconds).
const CLICK_WINDOW: f64 = 2.0;
const ANGRY_CLICKS: usize = 3;
const HIDE_CLICKS: usize = 5;
const ANGRY_FOR: f64 = 2.2;
/// Hiding: pull in, stay hidden, rise to peek with one eye, come back out.
const HIDE_IN: f64 = 0.45;
const HIDDEN_UNTIL: f64 = 2.25;
const PEEK_AT: f64 = 2.7;
const PEEK_UNTIL: f64 = 3.65;
const HIDE_END: f64 = 4.1;
const HIDE_LIFT: f32 = 26.;
const PEEK_LIFT: f32 = 9.;
/// Everything finished this long before it starts playing (seconds).
const BORED_AFTER: f64 = 120.;
/// It plays for `PLAY` seconds at the start of every `PLAY_EVERY`.
const PLAY: f64 = 7.;
const PLAY_EVERY: f64 = 20.;
const CELEBRATE_FOR: f64 = 1.8;
const STARTLE_FOR: f64 = 1.4;
/// A wink every `WINK_EVERY` seconds while working, lasting `WINK_FOR`.
const WINK_EVERY: f64 = 11.;
const WINK_FOR: f64 = 0.45;

#[derive(Default)]
pub struct Mood {
    clicks: Vec<f64>,
    angry_until: f64,
    hide_at: Option<f64>,
    done_since: Option<f64>,
    celebrate_at: Option<f64>,
    startle_at: Option<f64>,
    /// Last state seen per session, to catch the moment one changes.
    seen: HashMap<String, String>,
}

impl Mood {
    /// Notes the sessions' states at time `t`, catching sessions that just
    /// finished or just started waiting. `focus` is what the moon acts out.
    pub fn observe<'a>(
        &mut self,
        rows: impl IntoIterator<Item = (&'a str, &'a str)>,
        focus: &str,
        t: f64,
    ) {
        let mut seen = HashMap::new();
        for (id, state) in rows {
            if let Some(before) = self.seen.get(id) {
                if state == "done" && before != "done" && before != "inactive" {
                    self.celebrate_at = Some(t);
                }
                if state == "waiting" && before != "waiting" {
                    self.startle_at = Some(t);
                }
            }
            seen.insert(id.to_string(), state.to_string());
        }
        self.seen = seen;
        if focus == "done" {
            self.done_since.get_or_insert(t);
        } else {
            self.done_since = None;
        }
    }

    /// A click on the moon at time `t`.
    pub fn click(&mut self, t: f64) {
        if self.hide_at.is_some_and(|h| t - h < HIDE_END) {
            return;
        }
        self.clicks.retain(|c| t - c < CLICK_WINDOW);
        self.clicks.push(t);
        if self.clicks.len() >= HIDE_CLICKS {
            self.hide_at = Some(t);
            self.clicks.clear();
            self.angry_until = t + HIDE_IN;
        } else if self.clicks.len() >= ANGRY_CLICKS {
            self.angry_until = t + ANGRY_FOR;
        }
    }

    /// What the octopus does at time `t`. `look` is the direction from the
    /// head to a nearby mouse (unit length at most), when there is one.
    pub fn act(&self, focus: &str, t: f64, collapsed: bool, look: Option<Vec2>) -> Act {
        // A session that needs the user always shows plainly.
        if focus == "waiting" || focus == "error" {
            return match self.startle_at {
                Some(s) if focus == "waiting" && t - s < STARTLE_FOR => {
                    Act::Startled(((t - s) / STARTLE_FOR) as f32)
                }
                _ => Act::None,
            };
        }
        if let Some(h) = self.hide_at {
            let e = t - h;
            if e < HIDE_END {
                return hiding(e);
            }
        }
        if t < self.angry_until {
            return Act::Angry;
        }
        if !collapsed {
            return Act::None;
        }
        if let Some(c) = self.celebrate_at {
            if t - c < CELEBRATE_FOR {
                return Act::Celebrate(((t - c) / CELEBRATE_FOR) as f32);
            }
        }
        if let Some(d) = self.done_since {
            let bored = t - d - BORED_AFTER;
            if bored >= 0. && bored.rem_euclid(PLAY_EVERY) < PLAY {
                return Act::Bored((bored.rem_euclid(PLAY_EVERY) / PLAY) as f32);
            }
        }
        if let Some(v) = look {
            return Act::Look(v.x, v.y);
        }
        if island::is_working(focus) && t.rem_euclid(WINK_EVERY) < WINK_FOR {
            return Act::Wink;
        }
        Act::None
    }
}

fn hiding(e: f64) -> Act {
    let ease = |x: f64| {
        let x = x.clamp(0., 1.) as f32;
        x * x * (3. - 2. * x)
    };
    let (lift, peek) = if e < HIDE_IN {
        (HIDE_LIFT * ease(e / HIDE_IN), false)
    } else if e < HIDDEN_UNTIL {
        (HIDE_LIFT, false)
    } else if e < PEEK_AT {
        let x = ease((e - HIDDEN_UNTIL) / (PEEK_AT - HIDDEN_UNTIL));
        (HIDE_LIFT + (PEEK_LIFT - HIDE_LIFT) * x, true)
    } else if e < PEEK_UNTIL {
        (PEEK_LIFT, true)
    } else {
        (
            PEEK_LIFT * (1. - ease((e - PEEK_UNTIL) / (HIDE_END - PEEK_UNTIL))),
            false,
        )
    };
    Act::Hiding { lift, peek }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::vec2;

    #[test]
    fn clicks_annoy_then_hide() {
        let mut m = Mood::default();
        m.click(0.0);
        m.click(0.5);
        assert_eq!(m.act("editing", 0.6, true, None), Act::None);
        m.click(0.9);
        assert_eq!(m.act("editing", 1.0, true, None), Act::Angry);
        // Calms down on its own.
        assert_eq!(m.act("editing", 3.2, true, None), Act::None);
        // Slow clicks don't add up.
        m.click(10.0);
        m.click(13.0);
        m.click(16.0);
        assert_eq!(m.act("editing", 16.1, true, None), Act::None);

        for t in [20.0, 20.2, 20.4, 20.6, 20.8] {
            m.click(t);
        }
        assert!(
            matches!(m.act("editing", 21.5, true, None), Act::Hiding { lift, peek: false } if lift == HIDE_LIFT)
        );
        assert!(matches!(
            m.act("editing", 23.3, true, None),
            Act::Hiding { peek: true, .. }
        ));
        // Clicks while hiding are ignored, and it comes back out.
        m.click(22.0);
        assert_eq!(m.act("editing", 25.0, true, None), Act::None);
        // It also works with the widget open, where the clicks happen.
        m.click(30.0);
        m.click(30.1);
        m.click(30.2);
        assert_eq!(m.act("editing", 30.3, false, None), Act::Angry);
    }

    #[test]
    fn reacts_to_sessions_changing() {
        let mut m = Mood::default();
        // First sight of a session sets the baseline, nothing fires.
        m.observe([("a", "editing"), ("b", "done")], "editing", 0.);
        assert_eq!(m.act("editing", 1.0, true, None), Act::None);

        m.observe([("a", "done"), ("b", "done")], "done", 5.);
        assert!(matches!(m.act("done", 5.5, true, None), Act::Celebrate(_)));
        assert_eq!(m.act("done", 8., true, None), Act::None);

        m.observe([("a", "waiting"), ("b", "done")], "waiting", 9.);
        assert!(matches!(
            m.act("waiting", 9.5, true, None),
            Act::Startled(_)
        ));
        // Waiting shows plainly: no clicks or games on top of it.
        m.click(9.6);
        m.click(9.7);
        m.click(9.8);
        assert_eq!(m.act("waiting", 12., true, None), Act::None);
    }

    #[test]
    fn plays_when_bored_and_looks_at_the_mouse() {
        let mut m = Mood::default();
        m.observe([("a", "done")], "done", 0.);
        let near = Some(vec2(1., 0.));
        assert_eq!(m.act("done", 60., true, near), Act::Look(1., 0.));
        assert!(matches!(m.act("done", 121., true, None), Act::Bored(_)));
        // Rests between rounds.
        assert_eq!(m.act("done", 130., true, None), Act::None);
        assert!(matches!(m.act("done", 141., true, None), Act::Bored(_)));
        // Not while the widget is open.
        assert_eq!(m.act("done", 121., false, None), Act::None);
        // Any work resets the boredom.
        m.observe([("a", "editing")], "editing", 150.);
        m.observe([("a", "done")], "done", 151.);
        assert!(!matches!(m.act("done", 160., true, None), Act::Bored(_)));
    }

    #[test]
    fn winks_only_while_working() {
        let m = Mood::default();
        assert_eq!(m.act("editing", 22.1, true, None), Act::Wink);
        assert_eq!(m.act("editing", 23., true, None), Act::None);
        assert_eq!(m.act("done", 22.1, true, None), Act::None);
    }
}
