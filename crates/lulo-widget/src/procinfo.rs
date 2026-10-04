//! CPU and RAM of a session: the Claude Code process plus everything it
//! started (shells, builds, servers). Only measured while that session's
//! detail panel is open, at most every `EVERY`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const EVERY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    /// Share of the whole machine, 0..100. None on the first sample.
    pub cpu: Option<f32>,
    /// Working set in bytes.
    pub ram: u64,
}

#[derive(Default)]
pub struct Meter {
    /// Per root pid: last total CPU time (100 ns units), when, and the result.
    last: HashMap<u32, (u64, Instant, Usage)>,
}

impl Meter {
    /// Usage of `pid` and its descendants, re-measured every two seconds.
    pub fn sample(&mut self, pid: u32) -> Option<Usage> {
        let now = Instant::now();
        if let Some((_, at, usage)) = self.last.get(&pid) {
            if now.duration_since(*at) < EVERY {
                return Some(*usage);
            }
        }
        let (cpu_time, ram) = imp::measure(pid)?;
        let cpu = self.last.get(&pid).map(|(prev, at, _)| {
            let wall = now.duration_since(*at).as_secs_f64() * 1e7;
            let cores = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
            // A child that exits takes its time with it: never below zero.
            let used = cpu_time.saturating_sub(*prev) as f64;
            ((used / wall / cores) * 100.0).clamp(0.0, 100.0) as f32
        });
        let usage = Usage { cpu, ram };
        self.last.insert(pid, (cpu_time, now, usage));
        Some(usage)
    }

    /// Forgets sessions that are no longer shown.
    pub fn retain(&mut self, keep: impl Fn(u32) -> bool) {
        self.last.retain(|pid, _| keep(*pid));
    }
}

/// `root` and every process below it, from a child → parent map.
#[cfg_attr(not(windows), allow(dead_code))]
fn family(root: u32, parents: &HashMap<u32, u32>) -> HashSet<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&child, &parent) in parents {
        if child != parent {
            children.entry(parent).or_default().push(child);
        }
    }
    let mut seen = HashSet::from([root]);
    let mut stack = vec![root];
    while let Some(p) = stack.pop() {
        for &c in children.get(&p).into_iter().flatten() {
            if seen.insert(c) {
                stack.push(c);
            }
        }
    }
    seen
}

/// "410 MB", "1,2 GB".
pub fn bytes(n: u64) -> String {
    let mb = n as f64 / (1024.0 * 1024.0);
    if mb < 1000.0 {
        format!("{} MB", mb.round() as u64)
    } else {
        format!("{:.1} GB", mb / 1024.0).replace('.', ",")
    }
}

/// "12%", and one decimal below 10 so light work doesn't read as idle:
/// "0,4%". Task Manager uses the same scale (share of the whole machine).
pub fn percent(cpu: f32) -> String {
    if cpu < 9.95 {
        format!("{cpu:.1}%").replace('.', ",")
    } else {
        format!("{}%", cpu.round() as u32)
    }
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;

    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    fn parents() -> HashMap<u32, u32> {
        let mut map = HashMap::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap == INVALID_HANDLE_VALUE {
                return map;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e) != 0;
            while ok {
                map.insert(e.th32ProcessID, e.th32ParentProcessID);
                ok = Process32NextW(snap, &mut e) != 0;
            }
            CloseHandle(snap);
        }
        map
    }

    fn ticks(t: FILETIME) -> u64 {
        (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
    }

    /// (kernel + user time in 100 ns, working set bytes) of one process.
    fn one(pid: u32) -> Option<(u64, u64)> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let zero = FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            };
            let (mut c, mut x, mut k, mut u) = (zero, zero, zero, zero);
            let times = GetProcessTimes(h, &mut c, &mut x, &mut k, &mut u) != 0;
            let mut mem: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            mem.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let memory = GetProcessMemoryInfo(h, &mut mem, mem.cb) != 0;
            CloseHandle(h);
            (times || memory).then(|| {
                (
                    if times { ticks(k) + ticks(u) } else { 0 },
                    if memory { mem.WorkingSetSize as u64 } else { 0 },
                )
            })
        }
    }

    pub fn measure(root: u32) -> Option<(u64, u64)> {
        // The root must still be alive; a reused pid would be someone else.
        let first = one(root)?;
        let mut total = first;
        for pid in super::family(root, &parents()) {
            if pid == root {
                continue;
            }
            if let Some((t, m)) = one(pid) {
                total.0 += t;
                total.1 += m;
            }
        }
        Some(total)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn measure(_root: u32) -> Option<(u64, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_takes_every_descendant_once() {
        let parents = HashMap::from([(2, 1), (3, 2), (4, 2), (5, 9), (1, 0), (6, 6)]);
        let mut f: Vec<_> = family(1, &parents).into_iter().collect();
        f.sort();
        assert_eq!(f, [1, 2, 3, 4]);
        assert_eq!(family(6, &parents), HashSet::from([6]));
    }

    #[test]
    fn readable_sizes() {
        assert_eq!(bytes(410 * 1024 * 1024), "410 MB");
        assert_eq!(bytes(1258 * 1024 * 1024), "1,2 GB");
        assert_eq!(percent(0.04), "0,0%");
        assert_eq!(percent(0.36), "0,4%");
        assert_eq!(percent(12.4), "12%");
    }
}
