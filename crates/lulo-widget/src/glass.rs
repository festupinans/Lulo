//! Keeps Windows from framing the widget. The window is transparent and
//! only the moon and the chips are drawn, so nothing else may show: no
//! rounded corners, no border and no shadow around the window rectangle.

#[cfg(windows)]
pub fn apply(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };
    use winit::platform::windows::WindowExtWindows;

    // eframe asks for a drop shadow on undecorated windows. Turning it off
    // through winit keeps the rest of its frameless handling intact; turning
    // off the system frame drawing instead made the title bar flash on resize.
    if let Some(window) = cc.winit_window() {
        window.set_undecorated_shadow(false);
    }
    let Ok(RawWindowHandle::Win32(h)) = cc.window_handle().map(|h| h.as_raw()) else {
        return;
    };
    let hwnd = h.hwnd.get() as _;
    let set = |attribute: i32, value: u32| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute as u32,
            std::ptr::from_ref(&value).cast(),
            size_of_val(&value) as u32,
        );
    };
    // Windows 11 rounds the window rectangle and draws a thin border around
    // it, which showed as a frame around the open widget.
    set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND as u32);
    set(DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
}

#[cfg(not(windows))]
pub fn apply(_cc: &eframe::CreationContext<'_>) {}
