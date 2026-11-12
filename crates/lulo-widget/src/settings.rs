//! Widget settings in `%LOCALAPPDATA%\Lulo\widget.json`, meant to be edited
//! by hand.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::display::{Anchor, Edge};

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Animates the octopus. Off draws it still and keeps the widget idle.
    pub animate: bool,
    /// Minutes without hook events before a session shows as "Inactiva".
    pub inactive_minutes: u64,
    /// Hours without hook events before a session's file is deleted.
    pub forget_hours: u64,
    /// Windows notifications when a session waits, finishes or fails.
    pub notifications: bool,
    /// Lulo's sounds for the same events.
    pub sounds: bool,
    /// Also a soft sound when a new session opens.
    pub new_session_sound: bool,
    /// Hides the moon while a video, game or presentation is full screen.
    pub hide_fullscreen: bool,
    /// Monitor, in Lulo's list where the primary is 0. Gone = primary.
    pub monitor: usize,
    /// Where along the edge.
    pub anchor: Anchor,
    /// Top or bottom of the screen.
    pub edge: Edge,
    /// Size in percent of the original; 80 is a fifth smaller.
    pub size: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            animate: true,
            inactive_minutes: 5,
            forget_hours: 12,
            notifications: true,
            sounds: true,
            new_session_sound: false,
            hide_fullscreen: true,
            monitor: 0,
            anchor: Anchor::Center,
            edge: Edge::Top,
            size: 100,
        }
    }
}

impl Settings {
    pub fn inactive_secs(&self) -> u64 {
        self.inactive_minutes.max(1) * 60
    }

    pub fn forget_secs(&self) -> u64 {
        self.forget_hours.max(1) * 3600
    }

    /// egui's zoom factor for `size`, kept to a sane range.
    pub fn zoom(&self) -> f32 {
        self.size.clamp(MIN_SIZE, MAX_SIZE) as f32 / 100.0
    }

    pub fn load() -> Settings {
        file()
            .and_then(|f| fs::read_to_string(f).ok())
            .map(|t| Settings::parse(&t))
            .unwrap_or_default()
    }

    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let Ok(v) = serde_json::from_str::<Value>(text) else {
            return s;
        };
        if let Some(a) = v.get("animate").and_then(Value::as_bool) {
            s.animate = a;
        }
        if let Some(m) = v.get("inactive_minutes").and_then(Value::as_u64) {
            s.inactive_minutes = m;
        }
        if let Some(h) = v.get("forget_hours").and_then(Value::as_u64) {
            s.forget_hours = h;
        }
        let flag = |k: &str| v.get(k).and_then(Value::as_bool);
        if let Some(n) = flag("notifications") {
            s.notifications = n;
        }
        if let Some(n) = flag("sounds") {
            s.sounds = n;
        }
        if let Some(n) = flag("new_session_sound") {
            s.new_session_sound = n;
        }
        if let Some(h) = flag("hide_fullscreen") {
            s.hide_fullscreen = h;
        }
        if let Some(m) = v.get("monitor").and_then(Value::as_u64) {
            s.monitor = m as usize;
        }
        s.anchor = match v.get("anchor").and_then(Value::as_str) {
            Some("izquierda") => Anchor::Left,
            Some("derecha") => Anchor::Right,
            _ => Anchor::Center,
        };
        s.edge = match v.get("edge").and_then(Value::as_str) {
            Some("abajo") => Edge::Bottom,
            _ => Edge::Top,
        };
        if let Some(n) = v.get("size").and_then(Value::as_u64) {
            s.size = (n as u32).clamp(MIN_SIZE, MAX_SIZE);
        }
        s
    }

    /// Writes every field, so the file also documents the thresholds.
    pub fn save(&self) {
        let Some(path) = file() else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let v = json!({
            "animate": self.animate,
            "inactive_minutes": self.inactive_minutes,
            "forget_hours": self.forget_hours,
            "notifications": self.notifications,
            "sounds": self.sounds,
            "new_session_sound": self.new_session_sound,
            "hide_fullscreen": self.hide_fullscreen,
            "monitor": self.monitor,
            "anchor": match self.anchor {
                Anchor::Left => "izquierda",
                Anchor::Center => "centro",
                Anchor::Right => "derecha",
            },
            "edge": match self.edge {
                Edge::Top => "arriba",
                Edge::Bottom => "abajo",
            },
            "size": self.size,
        });
        let _ = fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default());
    }
}

/// Limits for `size`, in percent.
pub const MIN_SIZE: u32 = 50;
pub const MAX_SIZE: u32 = 200;

fn file() -> Option<PathBuf> {
    Some(dir()?.join("widget.json"))
}

/// Lulo's own folder: `%LOCALAPPDATA%\Lulo` on Windows.
pub fn dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    env("LULO_INSTALL_DIR").or_else(|| {
        if cfg!(windows) {
            env("LOCALAPPDATA").map(|d| d.join("Lulo"))
        } else {
            env("HOME").map(|h| h.join(".local").join("share").join("lulo"))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_defaults() {
        assert_eq!(Settings::parse("not json"), Settings::default());
        let s = Settings::parse(r#"{"animate": false, "inactive_minutes": 2}"#);
        assert!(!s.animate);
        assert_eq!(s.inactive_secs(), 120);
        assert_eq!(s.forget_secs(), 12 * 3600);
        assert!(s.notifications && s.sounds && !s.new_session_sound);
        let s = Settings::parse(r#"{"sounds": false, "monitor": 1, "anchor": "derecha"}"#);
        assert!(s.notifications && !s.sounds);
        assert_eq!((s.monitor, s.anchor), (1, Anchor::Right));
        // Zero would hide everything instantly; clamp to one unit.
        let s = Settings::parse(r#"{"inactive_minutes": 0, "forget_hours": 0}"#);
        assert_eq!((s.inactive_secs(), s.forget_secs()), (60, 3600));
        let s = Settings::parse(r#"{"edge": "abajo", "size": 80}"#);
        assert_eq!((s.edge, s.size, s.zoom()), (Edge::Bottom, 80, 0.8));
        assert_eq!(Settings::parse(r#"{"size": 5}"#).size, MIN_SIZE);
    }
}
