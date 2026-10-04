//! Adds or removes Lulo's hooks in the user's global `settings.json`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Every event the hook listens to. Tool events have no matcher, so they
/// fire for every tool and the binary decides the state.
pub const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
    "Notification",
    "Elicitation",
    "ElicitationResult",
    "PostModelSwitch",
    "SubagentStop",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "SessionEnd",
];

const HOOK_TIMEOUT_SECS: u64 = 5;

/// `~/.claude/settings.json`, honouring `CLAUDE_CONFIG_DIR`.
pub fn default_settings_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir).join("settings.json"));
    }
    let home = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(home)
        .filter(|v| !v.is_empty())
        .map(|h| PathBuf::from(h).join(".claude").join("settings.json"))
}

/// Where a double-click copies the hook: `%LOCALAPPDATA%\Lulo` on Windows,
/// `~/.local/share/lulo` elsewhere. `LULO_INSTALL_DIR` overrides it.
pub fn default_install_dir() -> Option<PathBuf> {
    let env = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(dir) = env("LULO_INSTALL_DIR") {
        return Some(dir);
    }
    if cfg!(windows) {
        return env("LOCALAPPDATA").map(|d| d.join("Lulo"));
    }
    env("HOME").map(|h| h.join(".local").join("share").join("lulo"))
}

/// Installs the hooks and drops the status line older versions of Lulo set.
/// Returns the backup of the previous settings file, if one existed.
pub fn install(settings_path: &Path, exe: &Path) -> io::Result<Option<PathBuf>> {
    let mut settings = read_settings(settings_path)?;
    add_hooks(&mut settings, &exe.to_string_lossy())?;
    remove_status_line(&mut settings);
    save(settings_path, &settings)
}

pub fn uninstall(settings_path: &Path) -> io::Result<Option<PathBuf>> {
    if !settings_path.exists() {
        return Ok(None);
    }
    let mut settings = read_settings(settings_path)?;
    let hooks = remove_hooks(&mut settings);
    if !remove_status_line(&mut settings) && !hooks {
        return Ok(None);
    }
    save(settings_path, &settings)
}

fn read_settings(path: &Path) -> io::Result<Value> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(json!({})),
        Ok(text) => serde_json::from_str(&text).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not valid JSON: {e}", path.display()),
            )
        }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e),
    }
}

fn save(path: &Path, settings: &Value) -> io::Result<Option<PathBuf>> {
    let backup = if path.exists() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let backup = path.with_extension(format!("json.lulo-backup-{stamp}"));
        fs::copy(path, &backup)?;
        Some(backup)
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        None
    };
    let mut text = serde_json::to_string_pretty(settings)?;
    text.push('\n');
    fs::write(path, text)?;
    Ok(backup)
}

/// Adds one Lulo handler per event, replacing any earlier Lulo handler so
/// installing twice (or from a new location) doesn't duplicate it.
pub fn add_hooks(settings: &mut Value, exe: &str) -> io::Result<()> {
    remove_hooks(settings);
    let root = settings
        .as_object_mut()
        .ok_or_else(|| invalid("settings.json must be a JSON object"))?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| invalid("\"hooks\" in settings.json must be an object"))?;

    for event in EVENTS {
        let groups = hooks.entry(*event).or_insert_with(|| json!([]));
        let groups = groups.as_array_mut().ok_or_else(|| {
            invalid(&format!(
                "\"hooks.{event}\" in settings.json must be an array"
            ))
        })?;
        groups.push(json!({
            "hooks": [{
                "type": "command",
                // Exec form (args set): Claude Code spawns the .exe directly,
                // with no Git Bash or PowerShell in between.
                "command": exe,
                "args": ["hook"],
                "timeout": HOOK_TIMEOUT_SECS,
            }]
        }));
    }
    Ok(())
}

/// Removes every Lulo handler, dropping groups and events it leaves empty.
/// Returns whether anything changed.
pub fn remove_hooks(settings: &mut Value) -> bool {
    let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) else {
        return false;
    };
    let mut changed = false;
    for groups in hooks.values_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                let before = handlers.len();
                handlers.retain(|h| !is_lulo_handler(h));
                changed |= handlers.len() != before;
            }
        }
        groups.retain(|g| {
            g.get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|h| !h.is_empty())
        });
    }
    if changed {
        hooks.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
        if hooks.is_empty() {
            if let Some(root) = settings.as_object_mut() {
                root.remove("hooks");
            }
        }
    }
    changed
}

/// Removes the plan usage status line older versions of Lulo installed.
pub fn remove_status_line(settings: &mut Value) -> bool {
    let Some(root) = settings.as_object_mut() else {
        return false;
    };
    if root.get("statusLine").is_some_and(is_lulo_status_line) {
        root.remove("statusLine");
        return true;
    }
    false
}

fn is_lulo_status_line(status_line: &Value) -> bool {
    let Some(command) = status_line.get("command").and_then(Value::as_str) else {
        return false;
    };
    let exe = command
        .trim()
        .strip_suffix("statusline")
        .unwrap_or("")
        .trim()
        .trim_matches('"');
    let name = crate::state::file_name(exe).to_ascii_lowercase();
    name == "lulo-hook" || name == "lulo-hook.exe"
}

fn is_lulo_handler(handler: &Value) -> bool {
    let Some(command) = handler.get("command").and_then(Value::as_str) else {
        return false;
    };
    let name = crate::state::file_name(command).to_ascii_lowercase();
    name == "lulo-hook" || name == "lulo-hook.exe"
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "C:\\Users\\me\\AppData\\Local\\Lulo\\lulo-hook.exe";

    #[test]
    fn install_adds_every_event_once() {
        let mut s = json!({});
        add_hooks(&mut s, EXE).unwrap();
        add_hooks(&mut s, EXE).unwrap();
        for event in EVENTS {
            let groups = s["hooks"][event].as_array().unwrap();
            assert_eq!(groups.len(), 1, "{event}");
            let h = &groups[0]["hooks"][0];
            assert_eq!(h["command"], EXE);
            assert_eq!(h["args"], json!(["hook"]));
            assert!(groups[0].get("matcher").is_none());
        }
    }

    #[test]
    fn install_keeps_existing_settings_and_hooks() {
        let mut s = json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "guard.sh" }] }]
            }
        });
        add_hooks(&mut s, EXE).unwrap();
        assert_eq!(s["model"], "opus");
        let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["hooks"][0]["command"], "guard.sh");
        // Key order is preserved: "model" stays first.
        assert_eq!(s.as_object().unwrap().keys().next().unwrap(), "model");
    }

    #[test]
    fn reinstall_from_new_path_replaces_old_one() {
        let mut s = json!({});
        add_hooks(&mut s, "/old/place/lulo-hook").unwrap();
        add_hooks(&mut s, EXE).unwrap();
        let stop = s["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], EXE);
    }

    #[test]
    fn uninstall_restores_original_shape() {
        let original = json!({
            "model": "opus",
            "hooks": {
                "PreToolUse": [{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "guard.sh" }] }]
            }
        });
        let mut s = original.clone();
        add_hooks(&mut s, EXE).unwrap();
        assert!(remove_hooks(&mut s));
        assert_eq!(s, original);

        let mut only_lulo = json!({ "model": "opus" });
        add_hooks(&mut only_lulo, EXE).unwrap();
        remove_hooks(&mut only_lulo);
        assert_eq!(only_lulo, json!({ "model": "opus" }));
    }

    #[test]
    fn uninstall_without_lulo_changes_nothing() {
        let mut s = json!({ "hooks": { "Stop": [] } });
        assert!(!remove_hooks(&mut s));
        assert_eq!(s, json!({ "hooks": { "Stop": [] } }));
    }

    #[test]
    fn old_status_line_is_removed() {
        let old = json!({ "type": "command", "command": "\"C:/Users/me/AppData/Local/Lulo/lulo-hook.exe\" statusline" });
        let mut s = json!({ "model": "opus", "statusLine": old });
        assert!(remove_status_line(&mut s));
        assert_eq!(s, json!({ "model": "opus" }));
    }

    #[test]
    fn users_own_status_line_is_kept() {
        let own = json!({ "type": "command", "command": "~/.claude/statusline.sh" });
        let mut s = json!({ "statusLine": own.clone() });
        assert!(!remove_status_line(&mut s));
        assert_eq!(s["statusLine"], own);
    }

    #[test]
    fn rejects_malformed_settings() {
        assert!(add_hooks(&mut json!([]), EXE).is_err());
        assert!(add_hooks(&mut json!({ "hooks": [] }), EXE).is_err());
        assert!(add_hooks(&mut json!({ "hooks": { "Stop": {} } }), EXE).is_err());
    }
}
