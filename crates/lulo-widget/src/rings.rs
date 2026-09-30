//! Plan usage around the octopus: an outer colored ring for the usage left
//! in the 5-hour window, and a thin white one for the time until it resets.
//! No text or numbers, just how full each ring is.
//!
//! The data comes from `_usage.json`, which `lulo-hook statusline` writes.

use std::fs;
use std::path::Path;

use eframe::egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};
use serde_json::Value;

use crate::octopus;

pub const USAGE_FILE: &str = "_usage.json";
const WINDOW_SECS: f64 = 5.0 * 3600.0;
/// How long the colored ring takes to refill after a reset.
const REFILL_SECS: f64 = 1.2;

const GREEN: Color32 = Color32::from_rgb(74, 222, 128);
const AMBER: Color32 = Color32::from_rgb(251, 191, 36);
const RED: Color32 = Color32::from_rgb(248, 113, 113);
const TIME: Color32 = Color32::from_rgb(232, 232, 238);

/// The 5-hour window as last reported by the status line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub used_percentage: f64,
    /// Unix seconds when the window resets.
    pub resets_at: Option<u64>,
}

impl Usage {
    pub fn parse(text: &str) -> Option<Usage> {
        let v: Value = serde_json::from_str(text).ok()?;
        let w = v.get("five_hour")?;
        Some(Usage {
            used_percentage: w.get("used_percentage")?.as_f64()?,
            resets_at: w.get("resets_at").and_then(Value::as_u64),
        })
    }

    pub fn load(dir: &Path) -> Option<Usage> {
        Usage::parse(&fs::read_to_string(dir.join(USAGE_FILE)).ok()?)
    }

    /// (usage left, time left), each 0..=1. Once the reset time passes the
    /// window starts over: all usage is back and the next window hasn't begun.
    pub fn fractions(&self, now: u64) -> (f32, f32) {
        if self.resets_at.is_some_and(|r| now >= r) {
            return (1.0, 0.0);
        }
        let left = (1.0 - self.used_percentage / 100.0).clamp(0.0, 1.0) as f32;
        let time = self.resets_at.map_or(0.0, |r| {
            ((r - now) as f64 / WINDOW_SECS).clamp(0.0, 1.0) as f32
        });
        (left, time)
    }
}

/// The colored ring's shown value, easing up to the real one when it jumps
/// (a reset refills it) instead of snapping.
#[derive(Default)]
pub struct Refill {
    seen: bool,
    from: f32,
    to: f32,
    since: f64,
}

impl Refill {
    pub fn value(&mut self, target: f32, t: f64, animate: bool) -> f32 {
        if !self.seen || (target - self.to).abs() > f32::EPSILON {
            // Only a refill animates; using Claude shortens the ring directly.
            self.from = if self.seen && animate && target > self.to + 0.05 {
                self.current(t)
            } else {
                target
            };
            self.to = target;
            self.since = t;
            self.seen = true;
        }
        self.current(t)
    }

    fn current(&self, t: f64) -> f32 {
        let k = ((t - self.since) / REFILL_SECS).clamp(0.0, 1.0) as f32;
        let k = k * k * (3.0 - 2.0 * k);
        self.from + (self.to - self.from) * k
    }
}

/// Color for how much usage is left: green with room, amber past half, red
/// near the end.
pub fn level(left: f32) -> Color32 {
    if left > 0.5 {
        GREEN
    } else if left > 0.2 {
        AMBER
    } else {
        RED
    }
}

/// Draws the rings and the octopus for `state` inside the square `rect`.
/// Designed on the same 120×120 canvas as the approved mockup.
pub fn paint(painter: &Painter, rect: Rect, left: f32, time: f32, state: &str, t: f64) {
    let k = rect.width().min(rect.height()) / 120.0;
    let c = rect.center();
    let at = |x: f32, y: f32| c + vec2(x - 60.0, y - 60.0) * k;

    let (r1, r2) = (52.0 * k, 41.0 * k);
    let track = if left <= 0.0 {
        RED.gamma_multiply(0.35)
    } else {
        Color32::from_white_alpha(33)
    };
    painter.circle_stroke(c, r1, Stroke::new(8.0 * k, track));
    let mut color = level(left);
    if left < 0.2 {
        // A soft heartbeat when almost nothing is left.
        let beat = 0.5 + 0.5 * (t * std::f64::consts::TAU / 1.6).sin() as f32;
        color = color.gamma_multiply(0.7 + 0.3 * beat);
    }
    // Glow under the arc, then the arc itself.
    arc(painter, c, r1, left, 13.0 * k, color.gamma_multiply(0.18));
    arc(painter, c, r1, left, 8.0 * k, color);

    painter.circle_stroke(c, r2, Stroke::new(3.0 * k, Color32::from_white_alpha(26)));
    arc(painter, c, r2, time, 3.0 * k, TIME);
    if time > 0.0 {
        let tip = point(c, r2, time);
        painter.circle_filled(tip, 4.6 * k, Color32::from_white_alpha(50));
        painter.circle_filled(tip, 2.6 * k, Color32::WHITE);
    }
    painter.line_segment(
        [at(60.0, 3.0), at(60.0, 9.0)],
        Stroke::new(1.6 * k, Color32::from_white_alpha(128)),
    );

    let mascot = Rect::from_min_size(at(34.0, 33.0), vec2(52.0, 52.0) * k);
    octopus::paint(painter, mascot, state, t);
}

/// Arc from 12 o'clock, clockwise, covering `frac` of the circle, with round
/// ends.
fn arc(painter: &Painter, c: Pos2, r: f32, frac: f32, width: f32, color: Color32) {
    if frac <= 0.0 {
        return;
    }
    let steps = ((frac * 96.0).ceil() as usize).max(2);
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| point(c, r, frac * i as f32 / steps as f32))
        .collect();
    let ends = [points[0], points[points.len() - 1]];
    painter.add(Shape::line(points, Stroke::new(width, color)));
    for p in ends {
        painter.circle_filled(p, width / 2.0, color);
    }
}

fn point(c: Pos2, r: f32, frac: f32) -> Pos2 {
    let a = std::f32::consts::TAU * frac - std::f32::consts::FRAC_PI_2;
    pos2(c.x + r * a.cos(), c.y + r * a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_hook_file() {
        let u = Usage::parse(
            r#"{"five_hour":{"used_percentage":34.4,"resets_at":20000},"seven_day":{"used_percentage":2},"ts":1}"#,
        )
        .unwrap();
        assert_eq!(u.used_percentage, 34.4);
        assert_eq!(u.resets_at, Some(20000));
        assert!(Usage::parse(r#"{"seven_day":{"used_percentage":2}}"#).is_none());
        assert!(Usage::parse("{").is_none());
    }

    #[test]
    fn fractions_follow_usage_and_reset() {
        let u = Usage {
            used_percentage: 25.0,
            resets_at: Some(10_000),
        };
        let (left, time) = u.fractions(10_000 - 9_000);
        assert!((left - 0.75).abs() < 1e-6);
        assert!((time - 0.5).abs() < 1e-6);
        // Past the reset everything is available again.
        assert_eq!(u.fractions(10_001), (1.0, 0.0));
        let over = Usage {
            used_percentage: 130.0,
            resets_at: None,
        };
        assert_eq!(over.fractions(0), (0.0, 0.0));
    }

    #[test]
    fn refill_eases_up_but_drops_at_once() {
        let mut r = Refill::default();
        assert_eq!(r.value(0.3, 0.0, true), 0.3);
        assert_eq!(r.value(0.2, 1.0, true), 0.2);
        let start = r.value(1.0, 2.0, true);
        assert!((start - 0.2).abs() < 1e-6);
        assert!(r.value(1.0, 2.6, true) > 0.5);
        assert_eq!(r.value(1.0, 4.0, true), 1.0);
        assert_eq!(Refill::default().value(1.0, 0.0, false), 1.0);
    }

    #[test]
    fn colors_by_level() {
        assert_eq!(level(0.8), GREEN);
        assert_eq!(level(0.4), AMBER);
        assert_eq!(level(0.1), RED);
    }
}
