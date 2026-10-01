//! Where the mouse is on the screen, even outside the widget's window, so
//! the collapsed octopus can look at it.

/// Mouse position in physical pixels of the virtual screen.
#[cfg(windows)]
pub fn screen_px() -> Option<(f32, f32)> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x as f32, p.y as f32))
}

/// Elsewhere the window only learns about the mouse when it is over it.
#[cfg(not(windows))]
pub fn screen_px() -> Option<(f32, f32)> {
    None
}
