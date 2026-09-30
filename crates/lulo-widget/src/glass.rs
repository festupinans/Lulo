//! Frosted glass on Windows: acrylic blur behind the window, cut to the
//! island's rounded shape with a window region so the blur doesn't show as
//! a rectangle. Elsewhere the window is simply transparent.

pub struct Glass {
    /// Acrylic blur is active behind the window.
    pub blur: bool,
    #[cfg(windows)]
    hwnd: Option<windows_sys::Win32::Foundation::HWND>,
}

#[cfg(windows)]
pub fn apply(window: &impl raw_window_handle::HasWindowHandle, blur: bool) -> Glass {
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };

    let hwnd = match window.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as windows_sys::Win32::Foundation::HWND),
        _ => None,
    };
    if let Some(hwnd) = hwnd {
        // The region gives the shape; Windows 11's own corners would add a
        // border around the old rectangle.
        let preference = DWMWCP_DONOTROUND;
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                std::ptr::from_ref(&preference).cast(),
                size_of_val(&preference) as u32,
            );
        }
    }
    let blur = blur && window_vibrancy::apply_acrylic(window, Some((26, 26, 38, 110))).is_ok();
    Glass { blur, hwnd }
}

#[cfg(windows)]
impl Glass {
    /// Clips the window to a rounded rectangle, in physical pixels.
    pub fn shape(&self, width: i32, height: i32, radius: i32) {
        use windows_sys::Win32::Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn};
        let Some(hwnd) = self.hwnd else {
            return;
        };
        unsafe {
            // The region's right and bottom edges are exclusive.
            let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, 2 * radius, 2 * radius);
            // The system owns the region from here on.
            SetWindowRgn(hwnd, region, 1);
        }
    }
}

#[cfg(not(windows))]
pub fn apply(_window: &impl raw_window_handle::HasWindowHandle, _blur: bool) -> Glass {
    Glass { blur: false }
}

#[cfg(not(windows))]
impl Glass {
    pub fn shape(&self, _width: i32, _height: i32, _radius: i32) {}
}
