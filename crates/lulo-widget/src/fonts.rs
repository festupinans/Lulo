//! Uses the system font (Segoe UI on Windows, fontconfig's sans-serif on
//! Linux) when available, keeping egui's bundled fonts as fallback for
//! icons and emoji. Also defines a semibold
//! family for names and headlines, which falls back to the regular font.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};

const SEMIBOLD: &str = "semibold";

/// Linux sans fonts (Noto, DejaVu...) run wider than Segoe UI, which the
/// chip layout was made for; a smaller size keeps names from being cut.
const TEXT_SCALE: f32 = if cfg!(windows) { 1.0 } else { 0.88 };

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size * TEXT_SCALE)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size * TEXT_SCALE, FontFamily::Name(SEMIBOLD.into()))
}

/// The regular and semibold font files to use, when there are any.
#[cfg(windows)]
fn files() -> (Option<PathBuf>, Option<PathBuf>) {
    let dir = std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts"));
    let at = |file: &str| dir.as_ref().map(|d| d.join(file));
    (at("segoeui.ttf"), at("seguisb.ttf"))
}

/// Whatever fontconfig picks for sans-serif. Few families ship a semibold,
/// and fontconfig answers that with the much wider bold, so ask for medium.
/// It only counts when it is a different file; a variable font would just
/// repeat the regular.
#[cfg(target_os = "linux")]
fn files() -> (Option<PathBuf>, Option<PathBuf>) {
    let find = |pattern: &str| {
        crate::linux::output("fc-match", &["--format=%{file}", pattern])
            .map(PathBuf::from)
            // egui reads TrueType and OpenType, not Type 1 or bitmap fonts.
            .filter(|p| {
                p.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
            })
    };
    let regular = find("sans-serif:weight=regular");
    let semibold = find("sans-serif:weight=medium").filter(|s| Some(s) != regular.as_ref());
    (regular, semibold)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn files() -> (Option<PathBuf>, Option<PathBuf>) {
    (None, None)
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let (regular_file, semibold_file) = files();
    let mut load = |name: &str, file: Option<PathBuf>| -> bool {
        let Some(bytes) = file.and_then(|f| std::fs::read(f).ok()) else {
            return false;
        };
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
        true
    };
    let regular = load("system", regular_file);
    let semibold = load("system-semibold", semibold_file);

    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if regular {
        proportional.insert(0, "system".into());
    }
    let mut strong = proportional.clone();
    if semibold {
        strong.insert(0, "system-semibold".into());
    }
    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), strong);
    ctx.set_fonts(fonts);
}
