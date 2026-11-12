//! What the Linux version needs from the desktop around it.
//!
//! Wayland lets no app place its own window or keep it on top, which is all
//! this widget does. So by default Lulo runs through XWayland, where both
//! work, and on Hyprland it also adds the window rules it needs (floating,
//! pinned to every workspace, no border, shadow or blur) to the running
//! session. `LULO_WAYLAND=1` keeps it on native Wayland; then the position
//! is up to the compositor's own rules.
//!
//! Everything else talks to small standard tools (`xrandr`, `hyprctl`,
//! `notify-send`, `pw-play`...) instead of linking their libraries, and
//! does nothing when one is missing.

use std::path::PathBuf;
use std::process::{Command, Stdio};

/// The window's X11 class and Wayland app id, which window rules match.
pub const APP_ID: &str = "lulo";

/// Runs `cmd` and returns its stdout when it succeeds.
pub fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Runs `cmd` and says whether it succeeded.
pub fn run(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn hyprland() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty())
}

/// Moves the widget onto XWayland unless `LULO_WAYLAND=1` or there is no
/// X server. Must run before the window (or any thread) exists.
pub fn prefer_x11() {
    let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
    let native = std::env::var("LULO_WAYLAND").is_ok_and(|v| v == "1");
    if set("WAYLAND_DISPLAY") && set("DISPLAY") && !native {
        // winit picks Wayland whenever this is set.
        std::env::remove_var("WAYLAND_DISPLAY");
    }
}

/// Lua rules for Hyprland 0.54 and later.
const HYPR_LUA: &str = r#"hl.window_rule({ match = { class = "^lulo$" }, float = true, pin = true, opaque = true, no_shadow = true, no_blur = true, no_dim = true, no_anim = true, rounding = 0, border_size = 0, no_initial_focus = true })"#;

/// The same for older, hyprlang-configured versions.
const HYPR_V2: [&str; 10] = [
    "float",
    "pin",
    "opaque",
    "noshadow",
    "noblur",
    "nodim",
    "noanim",
    "rounding 0",
    "noborder",
    "noinitialfocus",
];

/// Adds Lulo's window rules to the running Hyprland session. They last
/// until Hyprland reloads its config, and Lulo adds them again each start.
pub fn hyprland_rules() {
    if !hyprland() || run("hyprctl", &["eval", HYPR_LUA]) {
        return;
    }
    for rule in HYPR_V2 {
        let value = format!("{rule}, class:^({APP_ID})$");
        run("hyprctl", &["keyword", "windowrulev2", &value]);
    }
}

/// Writes a file Lulo ships inside the binary to its data folder, once.
pub fn data_file(name: &str, bytes: &[u8]) -> Option<PathBuf> {
    let path = crate::settings::dir()?.join(name);
    let same = std::fs::read(&path).is_ok_and(|old| old == bytes);
    if !same {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

/// Lulo's icon, for notifications.
pub fn icon() -> Option<PathBuf> {
    data_file("lulo.svg", include_bytes!("../../../assets/lulo.svg"))
}
