//! Frosted-glass backdrop on Windows: acrylic blur behind the window plus
//! Windows 11 rounded corners. Returns whether the blur is active so the
//! widget can draw a lighter tint over it.

#[cfg(windows)]
pub fn apply(window: &impl raw_window_handle::HasWindowHandle) -> bool {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    };

    let Ok(handle) = window.window_handle() else {
        return false;
    };
    if let RawWindowHandle::Win32(h) = handle.as_raw() {
        let preference = DWMWCP_ROUND;
        // Ignored before Windows 11; the blur still works there.
        unsafe {
            DwmSetWindowAttribute(
                h.hwnd.get() as _,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                std::ptr::from_ref(&preference).cast(),
                size_of_val(&preference) as u32,
            );
        }
    }
    window_vibrancy::apply_acrylic(window, Some((18, 18, 24, 120))).is_ok()
}

#[cfg(not(windows))]
pub fn apply(_window: &impl raw_window_handle::HasWindowHandle) -> bool {
    false
}
