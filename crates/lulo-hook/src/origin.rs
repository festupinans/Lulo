//! Where a session runs: the Claude Code process that ran this hook and how
//! it was started (desktop app, terminal, ...). The widget uses it to bring
//! the session's window to the front when its chip is clicked.
//!
//! Looking up the parent process costs a little, so it is only done when a
//! session starts or gets a new prompt, never on tool calls.

use serde_json::{json, Value};

/// Adds `claude_pid` and `entrypoint` to the record on the events that need it.
pub fn stamp(record: &mut Value, event: &str) {
    if !matches!(event, "SessionStart" | "UserPromptSubmit") {
        return;
    }
    let Some(rec) = record.as_object_mut() else {
        return;
    };
    if let Some(pid) = parent_pid() {
        rec.insert("claude_pid".into(), json!(pid));
    }
    if let Some(entry) = std::env::var("CLAUDE_CODE_ENTRYPOINT")
        .ok()
        .filter(|e| !e.is_empty())
    {
        rec.insert("entrypoint".into(), json!(entry));
    }
}

/// Claude Code runs the hook directly (exec form), so the parent is Claude.
#[cfg(windows)]
fn parent_pid() -> Option<u32> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let me = std::process::id();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            if entry.th32ProcessID == me {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        found
    }
}

#[cfg(unix)]
fn parent_pid() -> Option<u32> {
    Some(std::os::unix::process::parent_id())
}

#[cfg(not(any(windows, unix)))]
fn parent_pid() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_only_on_session_start_and_prompts() {
        let mut r = json!({ "state": "bash" });
        stamp(&mut r, "PreToolUse");
        assert!(r.get("claude_pid").is_none());
        stamp(&mut r, "UserPromptSubmit");
        assert!(r["claude_pid"].as_u64().is_some());
    }
}
