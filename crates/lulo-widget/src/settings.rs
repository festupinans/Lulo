//! Widget settings in `%LOCALAPPDATA%\Lulo\widget.json`: the window position
//! plus two thresholds the user can edit by hand.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub position: Option<[f32; 2]>,
    /// Minutes without hook events before a session shows as "Inactiva".
    pub inactive_minutes: u64,
    /// Hours without hook events before a session's file is deleted.
    pub forget_hours: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            position: None,
            inactive_minutes: 5,
            forget_hours: 12,
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
        let num = |k: &str| v.get(k).and_then(Value::as_f64);
        if let (Some(x), Some(y)) = (num("x"), num("y")) {
            if x.is_finite() && y.is_finite() {
                s.position = Some([x as f32, y as f32]);
            }
        }
        if let Some(m) = v.get("inactive_minutes").and_then(Value::as_u64) {
            s.inactive_minutes = m;
        }
        if let Some(h) = v.get("forget_hours").and_then(Value::as_u64) {
            s.forget_hours = h;
        }
        s
    }

    /// Writes every field, so the file also documents the thresholds.
    pub fn save(&self) {
        let Some(path) = file() else { return };
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let mut v = json!({
            "inactive_minutes": self.inactive_minutes,
            "forget_hours": self.forget_hours,
        });
        if let Some([x, y]) = self.position {
            v["x"] = json!(x);
            v["y"] = json!(y);
        }
        let _ = fs::write(path, serde_json::to_string_pretty(&v).unwrap_or_default());
    }
}

fn file() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let dir = env("LULO_INSTALL_DIR").or_else(|| {
        if cfg!(windows) {
            env("LOCALAPPDATA").map(|d| d.join("Lulo"))
        } else {
            env("HOME").map(|h| h.join(".local").join("share").join("lulo"))
        }
    })?;
    Some(dir.join("widget.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_defaults() {
        assert_eq!(Settings::parse("not json"), Settings::default());
        let s = Settings::parse(r#"{"x": 10, "y": 20.5, "inactive_minutes": 2}"#);
        assert_eq!(s.position, Some([10.0, 20.5]));
        assert_eq!(s.inactive_secs(), 120);
        assert_eq!(s.forget_secs(), 12 * 3600);
        // Zero would hide everything instantly; clamp to one unit.
        let s = Settings::parse(r#"{"inactive_minutes": 0, "forget_hours": 0}"#);
        assert_eq!((s.inactive_secs(), s.forget_secs()), (60, 3600));
    }
}
