//! Remembers where the widget was left, in `%LOCALAPPDATA%\Lulo\widget.json`.

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

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

pub fn load() -> Option<[f32; 2]> {
    let v: Value = serde_json::from_str(&fs::read_to_string(file()?).ok()?).ok()?;
    let x = v.get("x")?.as_f64()? as f32;
    let y = v.get("y")?.as_f64()? as f32;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

pub fn save(pos: [f32; 2]) {
    let Some(path) = file() else { return };
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, json!({ "x": pos[0], "y": pos[1] }).to_string());
}
