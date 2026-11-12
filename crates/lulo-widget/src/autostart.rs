//! Start with the session. On Windows, a value under HKCU\...\Run set with
//! `reg.exe` so the widget needs no registry crate. On Linux, an XDG
//! autostart entry in `~/.config/autostart`, which GNOME, KDE, XFCE and
//! friends run at login (Hyprland and other bare compositors need an
//! `exec-once` instead; see INSTALACION.md).

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

#[cfg(target_os = "linux")]
mod imp {
    use std::path::PathBuf;

    fn entry() -> Option<PathBuf> {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let config = env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?;
        Some(config.join("autostart").join("lulo.desktop"))
    }

    pub fn is_enabled() -> bool {
        entry().is_some_and(|p| p.exists())
    }

    pub fn set(enabled: bool) -> bool {
        let Some(path) = entry() else {
            return false;
        };
        if !enabled {
            return std::fs::remove_file(&path).is_ok() || !path.exists();
        }
        let Ok(exe) = std::env::current_exe() else {
            return false;
        };
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=Lulo\nComment=Estado de tus sesiones de Claude Code\nExec=\"{}\"\nIcon=lulo\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        path.parent()
            .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
            && std::fs::write(&path, text).is_ok()
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
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
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "linux"));
