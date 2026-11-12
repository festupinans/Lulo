//! Which monitor the moon sits on and where along its top edge, and whether
//! something full screen (a video, a game, a presentation) has the screen.
//!
//! Positions here are physical pixels in virtual-screen coordinates, the
//! way Windows reports them. egui wants `ViewportCommand::OuterPosition` in
//! points, so the caller divides by the *target* monitor's `scale`, not by
//! `ctx.pixels_per_point()`, which belongs to the monitor the window is on
//! right now. When the window crosses to a monitor with another scale,
//! winit rescales it on arrival, so place it again on the next frame.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Left,
    Center,
    Right,
}

/// Which screen edge the moon hangs from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edge {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Monitor {
    /// Left, top, right, bottom of the whole monitor in physical pixels.
    pub rect_px: (i32, i32, i32, i32),
    /// DPI scale, 1.0 = 96 dpi.
    pub scale: f32,
    pub primary: bool,
}

/// Gap between the moon and the side of the screen for `Left` and `Right`.
const MARGIN: f32 = 24.0;

/// Monitor `index`, or the primary when it is None or gone.
pub fn pick(monitors: &[Monitor], index: Option<usize>) -> Option<&Monitor> {
    index
        .and_then(|i| monitors.get(i))
        .or_else(|| monitors.iter().find(|m| m.primary))
        .or_else(|| monitors.first())
}

/// Top-left of the window in physical pixels for a window `size_px` big, at
/// `anchor` along `edge` of monitor `index` (None or out of range =
/// primary). Left/Right keep a margin of 24 logical px from the side.
pub fn place(
    monitors: &[Monitor],
    index: Option<usize>,
    anchor: Anchor,
    edge: Edge,
    size_px: (f32, f32),
) -> Option<(f32, f32)> {
    let m = pick(monitors, index)?;
    let (left, top, right, bottom) = m.rect_px;
    let (left, right) = (left as f32, right as f32);
    let margin = MARGIN * m.scale;
    let x = match anchor {
        Anchor::Left => left + margin,
        Anchor::Center => left + (right - left - size_px.0) / 2.0,
        Anchor::Right => right - margin - size_px.0,
    };
    let y = match edge {
        Edge::Top => top as f32,
        Edge::Bottom => (bottom as f32 - size_px.1).round(),
    };
    Some((x.max(left).round(), y))
}

/// Every monitor, primary first.
#[cfg(windows)]
pub fn monitors() -> Vec<Monitor> {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{LPARAM, RECT};
    use windows_sys::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };
    use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

    const MONITORINFOF_PRIMARY: u32 = 1;

    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        let list = unsafe { &mut *(data as *mut Vec<Monitor>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return 1;
        }
        let (mut dpi_x, mut dpi_y) = (0, 0);
        let hr = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = if hr >= 0 && dpi_x > 0 {
            dpi_x as f32 / 96.0
        } else {
            1.0
        };
        let r = info.rcMonitor;
        list.push(Monitor {
            rect_px: (r.left, r.top, r.right, r.bottom),
            scale,
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
        1
    }

    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(each),
            &mut list as *mut Vec<Monitor> as LPARAM,
        );
    }
    // Stable, so the rest keep the order Windows gave them.
    list.sort_by_key(|m| !m.primary);
    list
}

/// Every monitor as X11 sees it (also under XWayland), primary first.
///
/// X11 has one scale for the whole screen, so `scale` stays 1.0 and the
/// caller uses egui's own. The list is cached for a few seconds: the widget
/// asks again on every session change and `xrandr` is a process launch.
#[cfg(target_os = "linux")]
pub fn monitors() -> Vec<Monitor> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const FRESH: Duration = Duration::from_secs(10);
    static CACHE: Mutex<Option<(Instant, Vec<Monitor>)>> = Mutex::new(None);

    let Ok(mut cache) = CACHE.lock() else {
        return Vec::new();
    };
    if let Some((at, list)) = cache.as_ref() {
        if at.elapsed() < FRESH {
            return list.clone();
        }
    }
    let list = crate::linux::output("xrandr", &["--listmonitors"])
        .map(|text| parse_xrandr(&text))
        .unwrap_or_default();
    *cache = Some((Instant::now(), list.clone()));
    list
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn monitors() -> Vec<Monitor> {
    Vec::new()
}

/// Reads `xrandr --listmonitors`: one ` 0: +*DP-1 2560/597x1440/336+0+0  DP-1`
/// line per monitor, `*` marking the primary. XWayland marks none, so the
/// first one counts as primary then.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_xrandr(text: &str) -> Vec<Monitor> {
    let mut list: Vec<Monitor> = text
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            parts.next()?.strip_suffix(':')?;
            let name = parts.next()?;
            let geometry = parts.next()?;
            // 2560/597x1440/336+0+0: size/mm x size/mm +x +y.
            let (size, pos) = geometry.split_once('+')?;
            let (x, y) = pos.split_once('+')?;
            let (w, h) = size.split_once('x')?;
            let num = |s: &str| s.split('/').next()?.parse::<i32>().ok();
            let (w, h, x, y) = (num(w)?, num(h)?, num(x)?, num(y)?);
            Some(Monitor {
                rect_px: (x, y, x + w, y + h),
                scale: 1.0,
                primary: name.contains('*'),
            })
        })
        .collect();
    if !list.iter().any(|m| m.primary) {
        if let Some(first) = list.first_mut() {
            first.primary = true;
        }
    }
    list.sort_by_key(|m| !m.primary);
    list
}

/// Whether something full-screen or a presentation has the screen (a video,
/// a game, PowerPoint), so the widget can hide.
#[cfg(windows)]
pub fn busy_fullscreen() -> bool {
    use windows_sys::Win32::UI::Shell::SHQueryUserNotificationState;

    let mut state = 0;
    unsafe { SHQueryUserNotificationState(&mut state) >= 0 && is_busy(state) }
}

/// On Hyprland: whether the focused window is full screen. Other Linux
/// desktops don't say, so the widget never hides there.
#[cfg(target_os = "linux")]
pub fn busy_fullscreen() -> bool {
    if !crate::linux::hyprland() {
        return false;
    }
    crate::linux::output("hyprctl", &["activewindow", "-j"])
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|w| match &w["fullscreen"] {
            // 0 = no, 1 = maximized, 2 = full screen; older versions say true/false.
            serde_json::Value::Bool(b) => *b,
            v => v.as_u64() == Some(2),
        })
}

#[cfg(not(any(windows, target_os = "linux")))]
pub fn busy_fullscreen() -> bool {
    false
}

/// QUERY_USER_NOTIFICATION_STATE values that mean "keep out of the way":
/// QUNS_BUSY (a full-screen app), QUNS_RUNNING_D3D_FULL_SCREEN and
/// QUNS_PRESENTATION_MODE. Plain numbers so it can be tested anywhere.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_busy(state: i32) -> bool {
    matches!(state, 2..=4)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(rect_px: (i32, i32, i32, i32), scale: f32, primary: bool) -> Monitor {
        Monitor {
            rect_px,
            scale,
            primary,
        }
    }

    /// A 1.5x laptop to the left of a 1x primary.
    fn setup() -> Vec<Monitor> {
        vec![
            monitor((0, 0, 1920, 1080), 1.0, true),
            monitor((-2880, 0, 0, 1800), 1.5, false),
        ]
    }

    #[test]
    fn no_monitors() {
        assert_eq!(
            place(&[], None, Anchor::Center, Edge::Top, (100.0, 50.0)),
            None
        );
    }

    #[test]
    fn primary_by_default_and_out_of_range() {
        let ms = setup();
        let primary = Some((910.0, 0.0));
        assert_eq!(
            place(&ms, None, Anchor::Center, Edge::Top, (100.0, 50.0)),
            primary
        );
        assert_eq!(
            place(&ms, Some(7), Anchor::Center, Edge::Top, (100.0, 50.0)),
            primary
        );
    }

    #[test]
    fn primary_found_when_not_first() {
        let ms = vec![
            monitor((-2880, 0, 0, 1800), 1.5, false),
            monitor((0, 0, 1920, 1080), 1.0, true),
        ];
        assert_eq!(
            place(&ms, None, Anchor::Left, Edge::Top, (100.0, 50.0)),
            Some((24.0, 0.0))
        );
    }

    #[test]
    fn anchors() {
        let ms = setup();
        let at = |a| place(&ms, Some(0), a, Edge::Top, (100.0, 50.0));
        assert_eq!(at(Anchor::Left), Some((24.0, 0.0)));
        assert_eq!(at(Anchor::Center), Some((910.0, 0.0)));
        assert_eq!(at(Anchor::Right), Some((1796.0, 0.0)));
    }

    #[test]
    fn monitor_left_of_primary() {
        let ms = setup();
        let at = |a| place(&ms, Some(1), a, Edge::Top, (300.0, 75.0));
        assert_eq!(at(Anchor::Left), Some((-2844.0, 0.0)));
        assert_eq!(at(Anchor::Center), Some((-1590.0, 0.0)));
        assert_eq!(at(Anchor::Right), Some((-336.0, 0.0)));
    }

    #[test]
    fn wider_than_monitor_keeps_left_edge() {
        let ms = setup();
        assert_eq!(
            place(&ms, Some(0), Anchor::Center, Edge::Top, (3000.0, 50.0)),
            Some((0.0, 0.0))
        );
    }

    #[test]
    fn bottom_edge() {
        let ms = setup();
        assert_eq!(
            place(&ms, Some(0), Anchor::Center, Edge::Bottom, (100.0, 50.0)),
            Some((910.0, 1030.0))
        );
        assert_eq!(
            place(&ms, Some(1), Anchor::Left, Edge::Bottom, (300.0, 75.0)),
            Some((-2844.0, 1725.0))
        );
    }

    #[test]
    fn xrandr_monitors() {
        let text = "Monitors: 2\n \
            0: +eDP-1 2560/340x1600/220+0+0  eDP-1\n \
            1: +*DP-2 1920/527x1080/296+2560+0  DP-2\n";
        assert_eq!(
            parse_xrandr(text),
            [
                monitor((2560, 0, 4480, 1080), 1.0, true),
                monitor((0, 0, 2560, 1600), 1.0, false),
            ]
        );
        // XWayland marks no primary: the first one is.
        let xwayland = "Monitors: 1\n 0: +eDP-1 2560/340x1600/220+0+0  eDP-1\n";
        assert_eq!(
            parse_xrandr(xwayland),
            [monitor((0, 0, 2560, 1600), 1.0, true)]
        );
        assert_eq!(parse_xrandr("garbage"), []);
    }

    #[test]
    fn busy_states() {
        // QUNS_NOT_PRESENT through QUNS_APP.
        let busy: Vec<i32> = (1..=7).filter(|&s| is_busy(s)).collect();
        assert_eq!(busy, [2, 3, 4]);
    }
}
