//! Uses Segoe UI (the Windows system font) when available, keeping egui's
//! bundled fonts as fallback for icons and emoji.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

pub fn install(ctx: &egui::Context) {
    let Some(windir) = std::env::var_os("WINDIR") else {
        return;
    };
    let path = std::path::Path::new(&windir)
        .join("Fonts")
        .join("segoeui.ttf");
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "segoe".into());
    }
    ctx.set_fonts(fonts);
}
