//! Lulo: a glass panel docked to the right edge of the screen that shows
//! every Claude Code session and what it is doing right now.
//!
//! Collapsed it is a thin strip with the octopus mascot on top, acting out
//! the most urgent state, and a colored dot per session. Hovering it slides
//! the list out; hovering a session adds a card with its prompt, task list
//! and recent steps.
//!
//! It watches the folder `lulo-hook` writes to. Without animations it only
//! redraws when a file changes or the mouse is over it (plus a slow tick for
//! the age labels); with them, the octopus runs at a low frame rate while
//! collapsed and a smooth one while the panel is open.

// No console window behind the widget on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod fonts;
mod glass;
mod octopus;
mod sessions;
mod settings;
mod style;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, pos2, vec2, Align, CornerRadius, Layout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    UiBuilder, Vec2,
};
use notify::{RecursiveMode, Watcher};

use sessions::Session;
use settings::Settings;

/// Refresh for the age labels and the inactive state when no file changes.
const TICK: Duration = Duration::from_secs(15);
/// Re-reads the folder even without change events, to forget dead sessions.
const RELOAD: Duration = Duration::from_secs(60);
/// How long the panel stays open after the mouse leaves it.
const COLLAPSE_DELAY: Duration = Duration::from_millis(350);
/// Animation frame rates: the tiny collapsed octopus doesn't need many.
const FRAME_COLLAPSED: Duration = Duration::from_millis(1000 / 12);
const FRAME_EXPANDED: Duration = Duration::from_millis(1000 / 30);

const STRIP_W: f32 = 30.0;
/// Octopus sizes: collapsed strip, list header and session card.
const MASCOT_STRIP: f32 = 26.0;
const MASCOT_HEADER: f32 = 22.0;
const MASCOT_CARD: f32 = 64.0;
const LIST_W: f32 = 300.0;
const DETAIL_W: f32 = 330.0;
const PAD: f32 = 12.0;
const ROW_H: f32 = 30.0;
const DOT_STEP: f32 = 16.0;
const RADIUS: u8 = 12;
const PROJECT_W: f32 = 92.0;

fn main() -> eframe::Result {
    let settings = Settings::load();
    // Writes the defaults on first run so the settings are there to edit.
    settings.save();
    let viewport = egui::ViewportBuilder::default()
        .with_title("Lulo")
        .with_inner_size([STRIP_W, 80.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        // Out of the taskbar and Alt+Tab, and don't steal focus on launch.
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(false);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Lulo",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc, settings)))),
    )
}

struct App {
    dir: Option<PathBuf>,
    sessions: Vec<Session>,
    /// Set by the watcher thread; the UI thread reloads when it sees it.
    dirty: Arc<AtomicBool>,
    /// Kept alive for as long as the app runs.
    _watcher: Option<notify::RecommendedWatcher>,
    last_load: Option<Instant>,
    settings: Settings,
    autostart: bool,
    /// Acrylic blur is active behind the window.
    glass: bool,
    expanded: bool,
    last_inside: Option<Instant>,
    /// Session whose card is shown; stays while the mouse moves onto the card.
    hovered: Option<String>,
    /// Content heights measured last frame, used to size the window.
    list_h: f32,
    detail_h: f32,
    geometry: Option<(Pos2, Vec2)>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, settings: Settings) -> Self {
        let dir = sessions::status_dir();
        let dirty = Arc::new(AtomicBool::new(true));
        let watcher = dir
            .as_ref()
            .and_then(|d| watch(d, &dirty, cc.egui_ctx.clone()));
        fonts::install(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let glass = settings.glass && glass::apply(cc);
        App {
            dir,
            sessions: Vec::new(),
            dirty,
            _watcher: watcher,
            last_load: None,
            settings,
            autostart: autostart::is_enabled(),
            glass,
            expanded: false,
            last_inside: None,
            hovered: None,
            list_h: 0.0,
            detail_h: 0.0,
            geometry: None,
        }
    }

    /// Opens on hover and closes a moment after the mouse leaves.
    fn update_expanded(&mut self, ctx: &egui::Context) {
        let inside = ctx.input(|i| i.pointer.hover_pos().is_some());
        let menu_open = egui::Popup::is_any_open(ctx);
        if inside || menu_open {
            self.expanded = true;
            self.last_inside = Some(Instant::now());
        } else if self.expanded {
            let since = self.last_inside.map_or(COLLAPSE_DELAY, |t| t.elapsed());
            if since >= COLLAPSE_DELAY {
                self.expanded = false;
                self.hovered = None;
            } else {
                ctx.request_repaint_after(COLLAPSE_DELAY - since);
            }
        }
    }

    /// Docks the window to the right edge, centered on the list.
    fn place_window(&mut self, ctx: &egui::Context, width: f32, list_h: f32, detail: bool) {
        let screen = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(vec2(1920.0, 1080.0));
        let top = ((screen.y - list_h) / 2.0).max(0.0);
        let mut height = list_h;
        if detail {
            height = height.max(self.detail_h.min(screen.y - top - 8.0));
        }
        let pos = pos2(screen.x - width, top).round();
        let size = vec2(width, height).round();
        if self.geometry != Some((pos, size)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            self.geometry = Some((pos, size));
        }
    }
}

impl App {
    /// Next frame for the animation, or just the slow tick when it is off.
    fn schedule_repaint(&self, ctx: &egui::Context, frame: Duration) {
        ctx.request_repaint_after(if self.settings.animate { frame } else { TICK });
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
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        // Reading the files ourselves raises access events on some systems;
        // reacting to them would reload in a loop.
        if event.is_ok_and(|e| e.kind.is_access()) {
            return;
        }
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
        let ctx = ui.ctx().clone();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let stale = self.last_load.is_none_or(|t| t.elapsed() >= RELOAD);
        if self.dirty.swap(false, Ordering::Relaxed) || stale {
            if let Some(dir) = &self.dir {
                self.sessions = sessions::load(dir, now, self.settings.forget_secs());
            }
            self.last_load = Some(Instant::now());
        }
        self.update_expanded(&ctx);
        // Still octopus when animations are off: a fixed moment of each scene.
        let t = if self.settings.animate {
            ctx.input(|i| i.time)
        } else {
            0.0
        };

        // Rows that need attention stay on top; inactive ones sink.
        let inactive_secs = self.settings.inactive_secs();
        let mut rows: Vec<(&Session, &str)> = self
            .sessions
            .iter()
            .map(|s| (s, s.shown_state(now, inactive_secs)))
            .collect();
        rows.sort_by_key(|(_, state)| *state == "inactive");
        if !rows
            .iter()
            .any(|(s, _)| Some(&s.id) == self.hovered.as_ref())
        {
            self.hovered = None;
        }

        let full = ui.max_rect();
        let bg = if self.glass {
            style::GLASS_TINT
        } else {
            style::BACKGROUND
        };
        ui.painter().rect(
            full,
            CornerRadius::same(RADIUS),
            bg,
            Stroke::new(1.0, style::EDGE),
            StrokeKind::Inside,
        );

        // Right-click anywhere: autostart and close.
        let background = ui.interact(full, ui.id().with("bg"), Sense::click());
        background.context_menu(|ui| {
            if autostart::SUPPORTED
                && ui
                    .checkbox(&mut self.autostart, "Iniciar con Windows")
                    .changed()
            {
                autostart::set(self.autostart);
                self.autostart = autostart::is_enabled();
            }
            if ui.button("Cerrar Lulo").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });

        let urgent = octopus::most_urgent(rows.iter().map(|(_, state)| *state));
        if !self.expanded {
            let height = draw_strip(ui, full, &rows, urgent, t);
            self.place_window(&ctx, STRIP_W, height, false);
            self.schedule_repaint(&ctx, FRAME_COLLAPSED);
            return;
        }

        // List on the right, card for the hovered session on its left.
        let list_rect = Rect::from_min_max(pos2(full.right() - LIST_W, full.top()), full.max);
        let mut hovered = self.hovered.clone();
        let list = ui.scope_builder(UiBuilder::new().max_rect(list_rect.shrink(PAD)), |ui| {
            list_header(ui, rows.len(), urgent, t);
            if rows.is_empty() {
                ui.label(RichText::new("Sin sesiones de Claude Code").color(style::MUTED));
            }
            for (s, state) in &rows {
                let is_hovered = hovered.as_deref() == Some(s.id.as_str());
                if session_row(ui, s, state, now, is_hovered) {
                    hovered = Some(s.id.clone());
                }
            }
        });
        self.hovered = hovered;
        self.list_h = list.response.rect.height() + 2.0 * PAD;

        let card = self
            .hovered
            .as_ref()
            .and_then(|id| rows.iter().find(|(s, _)| &s.id == id));
        let width = if let Some((s, state)) = card {
            let card_rect =
                Rect::from_min_max(full.min, pos2(full.right() - LIST_W, full.bottom()));
            ui.painter().vline(
                card_rect.right(),
                card_rect.y_range().shrink(PAD),
                Stroke::new(1.0, style::EDGE),
            );
            let card = ui.scope_builder(UiBuilder::new().max_rect(card_rect.shrink(PAD)), |ui| {
                ui.set_width(DETAIL_W - 2.0 * PAD);
                detail_card(ui, s, state, now, t)
            });
            self.detail_h = card.response.rect.height() + 2.0 * PAD;
            LIST_W + DETAIL_W
        } else {
            LIST_W
        };
        self.place_window(&ctx, width, self.list_h, card.is_some());
        self.schedule_repaint(&ctx, FRAME_EXPANDED);
    }
}

/// Collapsed look: the octopus on top, then one colored dot per session.
/// Returns the height it needs.
fn draw_strip(ui: &egui::Ui, rect: Rect, rows: &[(&Session, &str)], urgent: &str, t: f64) -> f32 {
    let painter = ui.painter();
    let x = rect.center().x;
    let mascot = Rect::from_center_size(
        pos2(x, rect.top() + 6.0 + MASCOT_STRIP / 2.0),
        Vec2::splat(MASCOT_STRIP),
    );
    octopus::paint(painter, mascot, urgent, t);
    let first = mascot.bottom() + 10.0;
    for (i, (_, state)) in rows.iter().enumerate() {
        let y = first + i as f32 * DOT_STEP;
        let color = style::look(state).color;
        if *state == "waiting" {
            painter.circle_stroke(pos2(x, y), 6.5, Stroke::new(1.5, color.gamma_multiply(0.5)));
        }
        painter.circle_filled(pos2(x, y), 4.0, color);
    }
    let dots = rows.len().saturating_sub(1) as f32 * DOT_STEP;
    let bottom = if rows.is_empty() {
        mascot.bottom() + 6.0
    } else {
        first + dots + PAD
    };
    bottom - rect.top()
}

fn list_header(ui: &mut egui::Ui, count: usize, urgent: &str, t: f64) {
    ui.horizontal(|ui| {
        let (mascot, _) = ui.allocate_exact_size(Vec2::splat(MASCOT_HEADER), Sense::hover());
        octopus::paint(ui.painter(), mascot, urgent, t);
        ui.label(
            RichText::new("CLAUDE CODE")
                .size(11.0)
                .color(style::MUTED)
                .extra_letter_spacing(1.2),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let noun = if count == 1 { "sesión" } else { "sesiones" };
            ui.label(
                RichText::new(format!("{count} {noun}"))
                    .size(11.0)
                    .color(style::MUTED),
            );
        });
    });
    ui.add_space(4.0);
}

/// One session in the list. Returns whether the mouse is over it.
fn session_row(ui: &mut egui::Ui, s: &Session, state: &str, now: u64, highlighted: bool) -> bool {
    let look = style::look(state);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    if highlighted || response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), style::ROW_HOVER);
    }
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(rect.shrink2(vec2(6.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.spacing_mut().item_spacing.x = 8.0;
    row.label(RichText::new(look.icon).color(look.color));
    row.allocate_ui_with_layout(
        vec2(PROJECT_W, ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(PROJECT_W);
            ui.add(egui::Label::new(RichText::new(&s.project).color(style::TEXT)).truncate());
        },
    );
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(
            RichText::new(sessions::ago(now, s.ts))
                .size(11.0)
                .color(style::MUTED),
        );
        let (done, total) = s.task_counts();
        if total > 0 {
            ui.label(
                RichText::new(format!("{done}/{total}"))
                    .size(11.0)
                    .color(style::TEXT_DIM),
            );
        }
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.add(
                egui::Label::new(RichText::new(sessions::label(state)).color(look.color))
                    .truncate(),
            );
        });
    });
    response.hovered()
}

/// Everything known about one session: prompt, current step, tasks, history.
fn detail_card(ui: &mut egui::Ui, s: &Session, state: &str, now: u64, t: f64) {
    let look = style::look(state);
    ui.spacing_mut().item_spacing.y = 6.0;

    ui.horizontal(|ui| {
        let (mascot, _) = ui.allocate_exact_size(Vec2::splat(MASCOT_CARD), Sense::hover());
        octopus::paint(ui.painter(), mascot, state, t);
        ui.vertical(|ui| {
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(RichText::new(&s.project).size(16.0).color(style::TEXT))
                    .truncate(),
            );
            ui.label(RichText::new(sessions::label(state)).color(look.color));
        });
    });

    if let Some(prompt) = &s.prompt {
        section(ui, "PEDIDO");
        let mut job = egui::text::LayoutJob::single_section(
            prompt.clone(),
            egui::TextFormat::simple(egui::FontId::proportional(13.0), style::TEXT_DIM),
        );
        job.wrap.max_width = ui.available_width();
        job.wrap.max_rows = 4;
        ui.label(job);
    }

    section(ui, "AHORA");
    ui.horizontal(|ui| {
        ui.label(RichText::new(look.icon).color(look.color));
        let now_text = match &s.detail {
            Some(d) => format!("{} · {d}", sessions::label(state)),
            None => sessions::label(state).to_string(),
        };
        ui.add(egui::Label::new(RichText::new(now_text).color(style::TEXT)).truncate());
    });
    if let Some(started) = s.started {
        let since = sessions::ago(now, started);
        let text = if since == "ahora" {
            "Empezó hace menos de un minuto".to_string()
        } else {
            format!("Trabajando desde hace {since}")
        };
        ui.label(RichText::new(text).size(11.0).color(style::MUTED));
    }

    let (done, total) = s.task_counts();
    if total > 0 {
        ui.horizontal(|ui| {
            section(ui, "TAREAS");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{done} de {total}"))
                        .size(11.0)
                        .color(style::MUTED),
                );
            });
        });
        ui.add(
            egui::ProgressBar::new(done as f32 / total as f32)
                .desired_height(4.0)
                .fill(style::look("done").color),
        );
        for t in &s.tasks {
            let (icon, color) = match t.status.as_str() {
                "completed" => ("✔", style::look("done").color),
                "in_progress" => ("▶", style::ACCENT),
                _ => ("○", style::MUTED),
            };
            let text = RichText::new(&t.text);
            let text = match t.status.as_str() {
                "completed" => text.color(style::MUTED).strikethrough(),
                "in_progress" => text.color(style::TEXT),
                _ => text.color(style::TEXT_DIM),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).color(color));
                ui.add(egui::Label::new(text).wrap());
            });
        }
    } else if state != "inactive" && s.prompt.is_some() {
        ui.label(
            RichText::new("Esta sesión no usa lista de tareas.")
                .size(11.0)
                .color(style::MUTED),
        );
    }

    if !s.steps.is_empty() {
        section(ui, "PASOS RECIENTES");
        for step in s.steps.iter().rev().take(8) {
            let step_look = style::look(&step.state);
            ui.horizontal(|ui| {
                ui.label(RichText::new(step_look.icon).color(step_look.color.gamma_multiply(0.8)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(sessions::ago(now, step.ts))
                            .size(11.0)
                            .color(style::MUTED),
                    );
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let text = match &step.detail {
                            Some(d) => format!("{} · {d}", sessions::label(&step.state)),
                            None => sessions::label(&step.state).to_string(),
                        };
                        ui.add(
                            egui::Label::new(RichText::new(text).color(style::TEXT_DIM)).truncate(),
                        );
                    });
                });
            });
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(title)
            .size(10.5)
            .color(style::MUTED)
            .extra_letter_spacing(1.0),
    );
}
