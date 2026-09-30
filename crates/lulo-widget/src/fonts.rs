//! Uses Segoe UI (the Windows system font) when available, keeping egui's
//! bundled fonts as fallback for icons and emoji. Also defines a semibold
//! family for names and headlines, which falls back to the regular font.

use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};

const SEMIBOLD: &str = "semibold";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var_os("WINDIR").map(|w| std::path::Path::new(&w).join("Fonts"));
    let mut load = |name: &str, file: &str| -> bool {
        let Some(bytes) = dir.as_ref().and_then(|d| std::fs::read(d.join(file)).ok()) else {
            return false;
        };
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        true
    };
    let regular = load("segoe", "segoeui.ttf");
    let semibold = load("segoe-semibold", "seguisb.ttf");

    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if regular {
        proportional.insert(0, "segoe".into());
    }
    let mut strong = proportional.clone();
    if semibold {
        strong.insert(0, "segoe-semibold".into());
    }
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), strong);
    ctx.set_fonts(fonts);
}
