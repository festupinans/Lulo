//! Brings a session's window to the front when its chip is clicked: the
//! window of the nearest ancestor of the Claude Code process that owns one,
//! or else the Claude desktop app.

use std::collections::{HashMap, HashSet};

/// How far up the process tree to look before giving up.
const MAX_HOPS: usize = 16;

/// Console hosts: the shell's ancestors under Windows Terminal stop here.
const CONSOLE_HOSTS: [&str; 2] = ["OpenConsole.exe", "conhost.exe"];
const TERMINAL: &str = "WindowsTerminal.exe";
const DESKTOP_APP: &str = "Claude.exe";
/// The shell owns unrelated windows; reaching it means the walk went too far.
const SHELL: &str = "explorer.exe";

fn named(names: &HashMap<u32, String>, pid: u32, name: &str) -> bool {
    names
        .get(&pid)
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
}

/// The window of the lowest-numbered process called `name`, so the choice is
/// stable between clicks.
fn window_of_name<W: Copy>(
    name: &str,
    names: &HashMap<u32, String>,
    windows: &HashMap<u32, W>,
) -> Option<W> {
    windows
        .iter()
        .filter(|(pid, _)| named(names, **pid, name))
        .min_by_key(|(pid, _)| **pid)
        .map(|(_, w)| *w)
}

/// Chooses the window that hosts the session started by `start`.
#[cfg_attr(not(windows), allow(dead_code))]
fn pick<W: Copy>(
    start: Option<u32>,
    parents: &HashMap<u32, u32>,
    names: &HashMap<u32, String>,
    windows: &HashMap<u32, W>,
) -> Option<W> {
    let mut seen = HashSet::new();
    let mut console = false;
    let mut pid = start;
    while let Some(p) = pid {
        if seen.len() > MAX_HOPS || !seen.insert(p) || p == 0 || named(names, p, SHELL) {
            break;
        }
        if let Some(w) = windows.get(&p) {
            return Some(*w);
        }
        console |= CONSOLE_HOSTS.iter().any(|h| named(names, p, h));
        pid = parents.get(&p).copied();
    }
    console
        .then(|| window_of_name(TERMINAL, names, windows))
        .flatten()
        .or_else(|| window_of_name(DESKTOP_APP, names, windows))
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;

    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, INVALID_HANDLE_VALUE, LPARAM};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongW,
        GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE, WS_EX_TOOLWINDOW,
    };

    /// Every process's parent and executable name.
    fn processes() -> (HashMap<u32, u32>, HashMap<u32, String>) {
        let mut parents = HashMap::new();
        let mut names = HashMap::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return (parents, names);
            }
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut ok = Process32FirstW(snap, &mut entry);
            while ok != 0 {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(260);
                let pid = entry.th32ProcessID;
                parents.insert(pid, entry.th32ParentProcessID);
                names.insert(pid, String::from_utf16_lossy(&entry.szExeFile[..len]));
                ok = Process32NextW(snap, &mut entry);
            }
            CloseHandle(snap);
        }
        (parents, names)
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let windows = unsafe { &mut *(lparam as *mut HashMap<u32, isize>) };
        let shown = unsafe {
            IsWindowVisible(hwnd) != 0
                && GetWindow(hwnd, GW_OWNER).is_null()
                && GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0
                && GetWindowTextLengthW(hwnd) > 0
        };
        if shown {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            windows.entry(pid).or_insert(hwnd as isize);
        }
        1
    }

    /// The first visible, unowned, titled app window of each process.
    fn windows() -> HashMap<u32, isize> {
        let mut windows = HashMap::new();
        unsafe { EnumWindows(Some(collect), &raw mut windows as LPARAM) };
        windows
    }

    fn raise(hwnd: HWND) -> bool {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            if SetForegroundWindow(hwnd) != 0 {
                return true;
            }
            // Windows refused: borrow the foreground thread's input state.
            let ours = GetCurrentThreadId();
            let theirs = GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut());
            let attached = theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, 1) != 0;
            BringWindowToTop(hwnd);
            let ok = SetForegroundWindow(hwnd) != 0;
            if attached {
                AttachThreadInput(ours, theirs, 0);
            }
            ok
        }
    }

    pub fn bring_to_front(claude_pid: Option<u32>) -> bool {
        let (parents, names) = processes();
        let windows = windows();
        super::pick(claude_pid, &parents, &names, &windows).is_some_and(|w| raise(w as HWND))
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn bring_to_front(_claude_pid: Option<u32>) -> bool {
        false
    }
}

/// Brings the window that hosts this session to the front. Returns whether it found one.
pub fn bring_to_front(claude_pid: Option<u32>) -> bool {
    imp::bring_to_front(claude_pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the maps from `(pid, parent, name, window)` rows.
    fn tree(
        rows: &[(u32, u32, &str, Option<isize>)],
    ) -> (HashMap<u32, u32>, HashMap<u32, String>, HashMap<u32, isize>) {
        let mut parents = HashMap::new();
        let mut names = HashMap::new();
        let mut windows = HashMap::new();
        for &(pid, parent, name, window) in rows {
            parents.insert(pid, parent);
            names.insert(pid, name.to_string());
            if let Some(w) = window {
                windows.insert(pid, w);
            }
        }
        (parents, names, windows)
    }

    #[test]
    fn nearest_ancestor_with_a_window() {
        let (p, n, w) = tree(&[
            (10, 4, "explorer.exe", Some(1)),
            (20, 10, "Code.exe", Some(2)),
            (30, 20, "pwsh.exe", None),
            (40, 30, "claude.exe", None),
        ]);
        assert_eq!(pick(Some(40), &p, &n, &w), Some(2));
    }

    #[test]
    fn desktop_app_fallback() {
        let (p, n, w) = tree(&[
            (10, 4, "explorer.exe", Some(1)),
            (50, 10, "claude.EXE", Some(5)),
            (60, 70, "node.exe", None),
        ]);
        assert_eq!(pick(Some(60), &p, &n, &w), Some(5));
        // The walk never settles on the shell's own windows.
        assert_eq!(pick(Some(10), &p, &n, &w), Some(5));
    }

    #[test]
    fn windows_terminal_fallback() {
        let (p, n, w) = tree(&[
            (10, 4, "explorer.exe", Some(1)),
            (20, 10, "WindowsTerminal.exe", Some(2)),
            (50, 10, "Claude.exe", Some(5)),
            (30, 99, "OpenConsole.exe", None),
            (31, 30, "pwsh.exe", None),
            (32, 31, "claude.exe", None),
        ]);
        assert_eq!(pick(Some(32), &p, &n, &w), Some(2));
    }

    #[test]
    fn cycle_guard() {
        let (p, n, w) = tree(&[
            (1, 2, "a.exe", None),
            (2, 1, "b.exe", None),
            (50, 0, "Claude.exe", Some(5)),
        ]);
        assert_eq!(pick(Some(1), &p, &n, &w), Some(5));
        let (p, n, w) = tree(&[(1, 2, "a.exe", None), (2, 1, "b.exe", None)]);
        assert_eq!(pick(Some(1), &p, &n, &w), None);
    }

    #[test]
    fn no_start() {
        let (p, n, w) = tree(&[
            (20, 10, "Code.exe", Some(2)),
            (50, 10, "Claude.exe", Some(5)),
        ]);
        assert_eq!(pick(None, &p, &n, &w), Some(5));
        let (p, n, w) = tree(&[(20, 10, "Code.exe", Some(2))]);
        assert_eq!(pick(None, &p, &n, &w), None);
    }
}
