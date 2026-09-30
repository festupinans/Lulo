//! End-to-end: runs the real binary the way Claude Code does.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

const SESSION: &str = "0b6c1e3a-7f2d-4c55-9a51-2f1f4a9d8e01";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lulo-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_lulo-hook"))
}

/// Sends one event and returns the hook's stdout, which must stay empty.
fn send(dir: &Path, event: Value) {
    let mut child = bin()
        .arg("hook")
        .env("LULO_STATUS_DIR", dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty(), "hook must not print to stdout");
}

fn event(name: &str, extra: Value) -> Value {
    let mut v = json!({
        "session_id": SESSION,
        "transcript_path": "C:\\Users\\me\\.claude\\projects\\x\\t.jsonl",
        "cwd": "C:\\dev\\Lulo",
        "permission_mode": "default",
        "hook_event_name": name,
    });
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    v
}

fn read(dir: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(dir.join(format!("{SESSION}.json"))).unwrap()).unwrap()
}

#[test]
fn full_session_lifecycle() {
    let dir = temp_dir("lifecycle");

    send(&dir, event("SessionStart", json!({ "source": "startup" })));
    let r = read(&dir);
    assert_eq!(r["state"], "ready");
    assert_eq!(r["project"], "Lulo");
    assert!(r["ts"].as_u64().unwrap() > 0);

    send(&dir, event("UserPromptSubmit", json!({ "prompt": "hola" })));
    assert_eq!(read(&dir)["state"], "thinking");

    send(
        &dir,
        event(
            "PreToolUse",
            json!({ "tool_name": "Edit", "tool_input": { "file_path": "C:\\dev\\Lulo\\src\\main.rs" } }),
        ),
    );
    let r = read(&dir);
    assert_eq!(
        (r["state"].as_str(), r["detail"].as_str()),
        (Some("editing"), Some("main.rs"))
    );

    send(&dir, event("PostToolUse", json!({ "tool_name": "Edit" })));
    assert_eq!(read(&dir)["state"], "thinking");

    send(
        &dir,
        event(
            "PreToolUse",
            json!({ "tool_name": "Agent", "tool_input": { "description": "Explore" } }),
        ),
    );
    assert_eq!(read(&dir)["state"], "subagent");
    // The subagent's own tool calls keep the parent in "subagent".
    send(
        &dir,
        event(
            "PreToolUse",
            json!({ "tool_name": "Bash", "agent_id": "a1", "tool_input": { "command": "ls" } }),
        ),
    );
    assert_eq!(read(&dir)["state"], "subagent");

    send(
        &dir,
        event("PermissionRequest", json!({ "tool_name": "Bash" })),
    );
    assert_eq!(read(&dir)["state"], "waiting");

    send(&dir, event("Stop", json!({})));
    assert_eq!(read(&dir)["state"], "done");

    send(
        &dir,
        event("SessionEnd", json!({ "reason": "prompt_input_exit" })),
    );
    assert!(!dir.join(format!("{SESSION}.json")).exists());
    // No temp files left behind.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn bad_input_is_ignored_silently() {
    let dir = temp_dir("bad");
    let mut child = bin()
        .env("LULO_STATUS_DIR", &dir)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"not json").unwrap();
    assert!(child.wait().unwrap().success());

    send(
        &dir,
        json!({ "session_id": "../../evil", "hook_event_name": "Stop" }),
    );
    send(&dir, json!({ "hook_event_name": "Stop" }));
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn input_with_a_bom_is_accepted() {
    let dir = temp_dir("bom");
    let mut child = bin()
        .env("LULO_STATUS_DIR", &dir)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = b"\xEF\xBB\xBF".to_vec();
    input.extend_from_slice(
        br#"{"session_id":"bom-1","cwd":"C:\\x\\Demo","hook_event_name":"Stop"}"#,
    );
    child.stdin.take().unwrap().write_all(&input).unwrap();
    assert!(child.wait().unwrap().success());
    let r: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("bom-1.json")).unwrap()).unwrap();
    assert_eq!(
        (r["state"].as_str(), r["project"].as_str()),
        (Some("done"), Some("Demo"))
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn install_then_uninstall_round_trip() {
    let dir = temp_dir("install");
    let settings = dir.join("settings.json");
    let original = "{\n  \"model\": \"opus\"\n}\n";
    fs::write(&settings, original).unwrap();

    let out = bin()
        .args(["install", "--settings"])
        .arg(&settings)
        .args(["--exe", "C:\\Lulo\\lulo-hook.exe"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let s: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(
        s["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "C:\\Lulo\\lulo-hook.exe"
    );

    let out = bin()
        .args(["uninstall", "--settings"])
        .arg(&settings)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(fs::read_to_string(&settings).unwrap(), original);

    // Each write backs up the previous file.
    let backups = fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("lulo-backup")
        })
        .count();
    assert!(backups >= 1);
    fs::remove_dir_all(&dir).unwrap();
}
