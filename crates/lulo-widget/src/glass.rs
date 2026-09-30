//! Keeps Windows from framing the widget. The window is transparent and the
//! island draws its own translucent glass, so its rounded edges stay smooth.
//!
//! The system acrylic blur is not used: Windows fills the whole window
//! rectangle with it, ignoring the island's shape, and on some systems it
//! shows as a flat gray instead of a blur.

#[cfg(windows)]
pub fn apply(window: &impl raw_window_handle::HasWindowHandle) {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };

    let Ok(RawWindowHandle::Win32(h)) = window.window_handle().map(|h| h.as_raw()) else {
        return;
    };
    // Windows 11 would round the window rectangle and draw a border around it.
    let preference = DWMWCP_DONOTROUND;
    unsafe {
        DwmSetWindowAttribute(
            h.hwnd.get() as _,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            std::ptr::from_ref(&preference).cast(),
            size_of_val(&preference) as u32,
        );
    }
}

#[cfg(not(windows))]
pub fn apply(_window: &impl raw_window_handle::HasWindowHandle) {}
