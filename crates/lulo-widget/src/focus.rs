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
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
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

/// Linux: the process tree from `/proc`, and the windows from Hyprland or,
/// on X11, from `wmctrl`. A terminal emulator owns its window directly, so
/// the walk up from Claude usually stops at it.
#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashMap;

    use crate::linux;

    /// Every process's parent and command name.
    fn processes() -> (HashMap<u32, u32>, HashMap<u32, String>) {
        let mut parents = HashMap::new();
        let mut names = HashMap::new();
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return (parents, names);
        };
        for entry in dir.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse().ok()) else {
                continue;
            };
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                continue;
            };
            if let Some((name, parent)) = parse_stat(&stat) {
                parents.insert(pid, parent);
                names.insert(pid, name.to_string());
            }
        }
        (parents, names)
    }

    /// `/proc/<pid>/stat` is `pid (comm) state ppid ...`; the name may hold
    /// spaces and parentheses, so it ends at the last `)`.
    pub(super) fn parse_stat(stat: &str) -> Option<(&str, u32)> {
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat.get(open + 1..close)?;
        let mut rest = stat.get(close + 1..)?.split_whitespace();
        rest.next()?;
        Some((name, rest.next()?.parse().ok()?))
    }

    /// Hyprland: each process's first mapped window, by address.
    fn hyprland_windows() -> HashMap<u32, u64> {
        let clients = linux::output("hyprctl", &["clients", "-j"])
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        let mut windows = HashMap::new();
        for c in clients
            .as_ref()
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let pid = c["pid"].as_i64().and_then(|p| u32::try_from(p).ok());
            let address = c["address"]
                .as_str()
                .and_then(|a| u64::from_str_radix(a.trim_start_matches("0x"), 16).ok());
            if let (Some(pid), Some(address), true) = (pid, address, c["mapped"] != false) {
                windows.entry(pid).or_insert(address);
            }
        }
        windows
    }

    /// X11: `wmctrl -lp` lines are `0x01e00003  0 4321 host Title`.
    fn x11_windows() -> HashMap<u32, u64> {
        let mut windows = HashMap::new();
        for line in linux::output("wmctrl", &["-lp"])
            .unwrap_or_default()
            .lines()
        {
            let mut parts = line.split_whitespace();
            let id = parts
                .next()
                .and_then(|id| u64::from_str_radix(id.trim_start_matches("0x"), 16).ok());
            let pid = parts.nth(1).and_then(|p| p.parse().ok());
            if let (Some(id), Some(pid)) = (id, pid) {
                windows.entry(pid).or_insert(id);
            }
        }
        windows
    }

    fn raise(window: u64) -> bool {
        if linux::hyprland() {
            let lua = format!(r#"hl.dsp.focus({{ window = "address:0x{window:x}" }})"#);
            return linux::run("hyprctl", &["dispatch", &lua])
                || linux::run(
                    "hyprctl",
                    &["dispatch", "focuswindow", &format!("address:0x{window:x}")],
                );
        }
        linux::run("wmctrl", &["-ia", &format!("0x{window:x}")])
    }

    pub fn bring_to_front(claude_pid: Option<u32>) -> bool {
        let (parents, names) = processes();
        let windows = if linux::hyprland() {
            hyprland_windows()
        } else {
            x11_windows()
        };
        super::pick(claude_pid, &parents, &names, &windows).is_some_and(raise)
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
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

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_stat() {
        let parse = imp::parse_stat;
        assert_eq!(
            parse("4321 (claude) S 4300 4321 ..."),
            Some(("claude", 4300))
        );
        assert_eq!(parse("7 (a b) (c)) R 1 7 ..."), Some(("a b) (c)", 1)));
        assert_eq!(parse("garbage"), None);
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
