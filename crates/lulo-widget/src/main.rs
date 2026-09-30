//! Lulo: a small always-on-top window listing every Claude Code session and
//! what it is doing right now.
//!
//! It watches the folder `lulo-hook` writes to and only redraws when a file
//! changes (plus a slow tick for the "5 min" labels), so it idles at ~0 CPU.

// No console window behind the widget on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod sessions;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use notify::{RecursiveMode, Watcher};

use sessions::Session;

/// Refresh for the "hace N min" labels when no file changes.
const TICK: Duration = Duration::from_secs(15);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Lulo")
            .with_inner_size([300.0, 140.0])
            .with_min_inner_size([200.0, 60.0])
            .with_always_on_top(),
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.dirty.swap(false, Ordering::Relaxed) {
            if let Some(dir) = &self.dir {
                self.sessions = sessions::load(dir);
            }
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());

        egui::CentralPanel::default().show(ui, |ui| {
            if self.sessions.is_empty() {
                ui.weak("Sin sesiones de Claude Code activas");
                return;
            }
            egui::Grid::new("sessions")
                .num_columns(3)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    for s in &self.sessions {
                        ui.strong(&s.project);
                        let state = ui.label(s.label());
                        if let Some(detail) = &s.detail {
                            state.on_hover_text(detail);
                        }
                        ui.weak(sessions::ago(now, s.ts));
                        ui.end_row();
                    }
                });
        });

        ui.ctx().request_repaint_after(TICK);
    }
}
