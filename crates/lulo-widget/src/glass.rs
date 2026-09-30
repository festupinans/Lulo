//! Keeps Windows from framing the widget. The window is transparent and
//! only the moon and the chips are drawn, so nothing else may show: no
//! rounded corners, no border and no shadow around the window rectangle.

#[cfg(windows)]
pub fn apply(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMNCRP_DISABLED, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_NCRENDERING_POLICY, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };

    let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else {
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
    // Windows 11 rounds the window rectangle and draws a thin border and a
    // shadow around it, which showed as a frame around the open widget.
    set(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND as u32);
    set(DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE);
    // No frame drawing by the system at all, which also drops the shadow.
    set(DWMWA_NCRENDERING_POLICY, DWMNCRP_DISABLED as u32);
}

#[cfg(not(windows))]
pub fn apply(_window: &impl raw_window_handle::HasWindowHandle) {}
