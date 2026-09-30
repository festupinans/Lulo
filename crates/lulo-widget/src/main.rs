//! Lulo: a small always-on-top window listing every Claude Code session and
//! what it is doing right now.
//!
//! It watches the folder `lulo-hook` writes to and only redraws when a file
//! changes (plus a slow tick for the "5 min" labels), so it idles at ~0 CPU.

// No console window behind the widget on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod position;
mod sessions;
mod style;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Sense};
use notify::{RecursiveMode, Watcher};

use sessions::Session;

/// Refresh for the "5 min" labels when no file changes.
const TICK: Duration = Duration::from_secs(15);
const WIDTH: f32 = 320.0;
const MARGIN: f32 = 8.0;
const PROJECT_WIDTH: f32 = 96.0;

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Lulo")
        .with_inner_size([WIDTH, 40.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        // Out of the taskbar and Alt+Tab, and don't steal focus on launch.
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(false);
    if let Some(pos) = position::load() {
        viewport = viewport.with_position(pos);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native("Lulo", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

struct App {
    dir: Option<PathBuf>,
    sessions: Vec<Session>,
    /// Set by the watcher thread; the UI thread reloads when it sees it.
    dirty: Arc<AtomicBool>,
    /// Kept alive for as long as the app runs.
    _watcher: Option<notify::RecommendedWatcher>,
    saved_pos: Option<egui::Pos2>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let dir = sessions::status_dir();
        let dirty = Arc::new(AtomicBool::new(true));
        let watcher = dir
            .as_ref()
            .and_then(|d| watch(d, &dirty, cc.egui_ctx.clone()));
        App {
            dir,
            sessions: Vec::new(),
            dirty,
            _watcher: watcher,
            saved_pos: position::load().map(egui::Pos2::from),
        }
    }

    /// Saves the window position once a drag has finished.
    fn remember_position(&mut self, ctx: &egui::Context) {
        let (rect, pointer_down) = ctx.input(|i| (i.viewport().outer_rect, i.pointer.any_down()));
        let Some(rect) = rect else { return };
        if !pointer_down && self.saved_pos != Some(rect.min) {
            position::save([rect.min.x, rect.min.y]);
            self.saved_pos = Some(rect.min);
        }
    }
}

/// Watches `dir` (creating it if needed) and wakes the UI on any change.
fn watch(
    dir: &PathBuf,
    dirty: &Arc<AtomicBool>,
    ctx: egui::Context,
) -> Option<notify::RecommendedWatcher> {
    std::fs::create_dir_all(dir).ok()?;
    let dirty = Arc::clone(dirty);
    let mut watcher = notify::recommended_watcher(move |_: notify::Result<notify::Event>| {
        dirty.store(true, Ordering::Relaxed);
        ctx.request_repaint();
    })
    .ok()?;
    watcher.watch(dir, RecursiveMode::NonRecursive).ok()?;
    Some(watcher)
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.dirty.swap(false, Ordering::Relaxed) {
            if let Some(dir) = &self.dir {
                self.sessions = sessions::load(dir);
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let ctx = ui.ctx().clone();

        let frame = egui::Frame::new()
            .fill(style::BACKGROUND)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(MARGIN);
        let panel = egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            // The whole window is a drag handle, and right-click closes it.
            let background =
                ui.interact(ui.max_rect(), ui.id().with("drag"), Sense::click_and_drag());
            if background.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            background.context_menu(|ui| {
                if ui.button("Cerrar Lulo").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });

            let top = ui.cursor().top();
            if self.sessions.is_empty() {
                ui.label(RichText::new("Sin sesiones de Claude Code").color(Color32::GRAY));
            }
            for s in &self.sessions {
                session_row(ui, s, now);
            }
            ui.cursor().top() - top
        });

        // Grow or shrink the window to fit the rows.
        let height = (panel.inner + 2.0 * MARGIN).ceil();
        let current = ctx.input(|i| i.viewport().inner_rect.map(|r| r.height()));
        if current.is_none_or(|h| (h - height).abs() > 1.0) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(WIDTH, height)));
        }

        self.remember_position(&ctx);
        ctx.request_repaint_after(TICK);
    }
}

fn session_row(ui: &mut egui::Ui, s: &Session, now: u64) {
    let look = style::look(&s.state);
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_sized(
            [16.0, 18.0],
            egui::Label::new(RichText::new(look.icon).color(look.color)),
        );
        // Fixed-width, left-aligned project column so the states line up.
        ui.allocate_ui_with_layout(
            egui::vec2(PROJECT_WIDTH, 18.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_min_width(PROJECT_WIDTH);
                ui.add(
                    egui::Label::new(
                        RichText::new(&s.project)
                            .strong()
                            .color(Color32::from_gray(230)),
                    )
                    .truncate(),
                );
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(sessions::ago(now, s.ts))
                    .small()
                    .color(Color32::from_gray(120)),
            );
            let mut text = egui::text::LayoutJob::default();
            text.append(s.label(), 0.0, text_format(ui, look.color));
            if let Some(detail) = &s.detail {
                text.append(
                    &format!("  {detail}"),
                    0.0,
                    text_format(ui, Color32::from_gray(140)),
                );
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(egui::Label::new(text).truncate());
            });
        });
    });
    if let Some(detail) = &s.detail {
        row.response.on_hover_text(detail);
    }
}

fn text_format(ui: &egui::Ui, color: Color32) -> egui::TextFormat {
    egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        color,
        ..Default::default()
    }
}
