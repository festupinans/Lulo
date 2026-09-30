//! Colors per session state.

use eframe::egui::Color32;

pub struct Look {
    pub color: Color32,
}

pub fn look(state: &str) -> Look {
    let [r, g, b] = match state {
        "thinking" => [167, 139, 250],
        "editing" => [96, 165, 250],
        "bash" => [45, 212, 191],
        "reading" => [125, 211, 252],
        "subagent" => [244, 114, 182],
        "waiting" => [251, 191, 36],
        "background" => [129, 140, 248],
        "done" => [74, 222, 128],
        "error" => [248, 113, 113],
        "inactive" => [100, 116, 139],
        // "ready", "tool" and anything unknown.
        _ => [148, 163, 184],
    };
    Look {
        color: Color32::from_rgb(r, g, b),
    }
}

pub const TEXT: Color32 = Color32::from_rgb(236, 236, 241);
