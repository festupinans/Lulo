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
        _ => ("?", [148, 163, 184]),
    };
    Look {
        icon,
        color: Color32::from_rgb(r, g, b),
    }
}

/// Window background: dark and slightly see-through.
pub const BACKGROUND: Color32 = Color32::from_rgba_premultiplied(18, 18, 22, 225);
