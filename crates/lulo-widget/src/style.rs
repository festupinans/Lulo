//! Colors and icons per session state.

use eframe::egui::Color32;

pub struct Look {
    pub icon: &'static str,
    pub color: Color32,
}

pub fn look(state: &str) -> Look {
    let (icon, [r, g, b]) = match state {
        "ready" => ("○", [148, 163, 184]),
        "thinking" => ("💭", [167, 139, 250]),
        "editing" => ("✏", [96, 165, 250]),
        "bash" => ("⌨", [45, 212, 191]),
        "reading" => ("🔍", [125, 211, 252]),
        "subagent" => ("👥", [244, 114, 182]),
        "tool" => ("🔧", [148, 163, 184]),
        "waiting" => ("✋", [251, 191, 36]),
        "done" => ("✔", [74, 222, 128]),
        "error" => ("⚠", [248, 113, 113]),
        "inactive" => ("🌙", [100, 116, 139]),
        _ => ("?", [148, 163, 184]),
    };
    Look {
        icon,
        color: Color32::from_rgb(r, g, b),
    }
}

/// Window background without the Windows blur: dark and slightly see-through.
pub const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(15, 15, 20, 235);
/// Tint over the acrylic blur, light enough to let it show through.
pub const GLASS_TINT: Color32 = Color32::from_rgba_premultiplied(10, 10, 14, 150);
/// Thin highlight around the panel, like the edge of a glass pane.
pub const EDGE: Color32 = Color32::from_rgba_premultiplied(40, 40, 44, 40);
pub const ROW_HOVER: Color32 = Color32::from_rgba_premultiplied(14, 14, 16, 16);
pub const TEXT: Color32 = Color32::from_rgb(236, 236, 241);
pub const TEXT_DIM: Color32 = Color32::from_rgb(180, 182, 192);
pub const MUTED: Color32 = Color32::from_rgb(120, 124, 138);
pub const ACCENT: Color32 = Color32::from_rgb(167, 139, 250);
