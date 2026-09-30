//! "Iniciar con Windows": a value under HKCU\...\Run, set with `reg.exe` so
//! the widget needs no registry crate.

#[cfg(windows)]
mod imp {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const NAME: &str = "Lulo";
    /// Keeps reg.exe from flashing a console window.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn reg(args: &[&str]) -> bool {
        Command::new("reg")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .is_ok_and(|s| s.success())
    }

    pub fn is_enabled() -> bool {
        reg(&["query", KEY, "/v", NAME])
    }

    pub fn set(enabled: bool) -> bool {
        if !enabled {
            return reg(&["delete", KEY, "/v", NAME, "/f"]);
        }
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let value = format!("\"{}\"", exe.display());
        reg(&["add", KEY, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"])
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }

    pub fn set(_enabled: bool) -> bool {
        false
    }
}

pub use imp::{is_enabled, set};

/// Whether this platform supports the option at all.
pub const SUPPORTED: bool = cfg!(windows);
