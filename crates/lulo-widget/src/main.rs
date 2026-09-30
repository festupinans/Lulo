//! Lulo: a glass island at the top center of the screen that shows every
//! Claude Code session and what it is doing right now.
//!
//! Collapsed it is a small pill: the octopus acting out what most needs you,
//! one sentence about it and a colored dot per session. Hovering it opens
//! the island: the plan usage rings, one chip per session, and below them a
//! summary or the details of the chip under the mouse.
//!
//! It watches the folder `lulo-hook` writes to. Without animations it only
//! redraws when a file changes or the mouse is over it (plus a slow tick for
//! the age labels); with them, the octopus runs at a low frame rate while
//! collapsed and a smooth one while the island is open.

// No console window behind the widget on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
mod fonts;
mod glass;
mod island;
mod octopus;
mod rings;
mod sessions;
mod settings;
mod style;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Align, Color32, CornerRadius, FontId, Layout, Painter, Pos2,
    Rect, RichText, Sense, Stroke, StrokeKind, TextFormat, UiBuilder, Vec2,
};
use notify::{RecursiveMode, Watcher};

use island::{Line, Tone};
use sessions::Session;
use settings::Settings;

/// Refresh for the age labels and the inactive state when no file changes.
const TICK: Duration = Duration::from_secs(15);
/// Re-reads the folder even without change events, to forget dead sessions.
const RELOAD: Duration = Duration::from_secs(60);
/// How long the island stays open after the mouse leaves it.
const COLLAPSE_DELAY: Duration = Duration::from_millis(350);
/// Animation frame rates: the tiny collapsed octopus doesn't need many.
const FRAME_COLLAPSED: Duration = Duration::from_millis(1000 / 12);
const FRAME_EXPANDED: Duration = Duration::from_millis(1000 / 30);

/// Gap between the island and the top of the screen.
const TOP: f32 = 8.0;

// Collapsed pill.
const PILL_H: f32 = 38.0;
const PILL_OCTOPUS: f32 = 28.0;
const PILL_TEXT_MAX: f32 = 380.0;
const DOT: f32 = 7.0;
const DOT_GAP: f32 = 5.0;
const MAX_DOTS: usize = 8;

// Open island.
const RADIUS: f32 = 24.0;
const PAD: f32 = 12.0;
const RINGS: f32 = 80.0;
const COLUMNS: usize = 4;
const CHIP_W: f32 = 168.0;
const CHIP_H: f32 = 62.0;
const CHIP_GAP: f32 = 8.0;
const CHIP_OCTOPUS: f32 = 34.0;
const MIN_W: f32 = 460.0;
const MAX_STEPS: usize = 5;

fn main() -> eframe::Result {
    let settings = Settings::load();
    // Writes the defaults on first run so the settings are there to edit.
    settings.save();
    let viewport = egui::ViewportBuilder::default()
        .with_title("Lulo")
        .with_inner_size([200.0, PILL_H])
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
    usage: Option<rings::Usage>,
    refill: rings::Refill,
    /// Set by the watcher thread; the UI thread reloads when it sees it.
    dirty: Arc<AtomicBool>,
    /// Kept alive for as long as the app runs.
    _watcher: Option<notify::RecommendedWatcher>,
    last_load: Option<Instant>,
    settings: Settings,
    autostart: bool,
    glass: glass::Glass,
    expanded: bool,
    last_inside: Option<Instant>,
    /// Session whose details are shown; stays while the mouse moves down to them.
    hovered: Option<String>,
    /// Height of the details measured last frame, used to size the window.
    detail_h: f32,
    geometry: Option<(Pos2, Vec2, f32)>,
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
        let glass = glass::apply(cc, settings.glass);
        App {
            dir,
            sessions: Vec::new(),
            usage: None,
            refill: rings::Refill::default(),
            dirty,
            _watcher: watcher,
            last_load: None,
            settings,
            autostart: autostart::is_enabled(),
            glass,
            expanded: false,
            last_inside: None,
            hovered: None,
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

    /// Centers the window at the top of the screen and cuts it to the
    /// island's shape.
    fn place_window(&mut self, ctx: &egui::Context, size: Vec2, radius: f32) {
        let screen = ctx
            .input(|i| i.viewport().monitor_size)
            .unwrap_or(vec2(1920.0, 1080.0));
        let size = size.round();
        let pos = pos2(((screen.x - size.x) / 2.0).max(0.0), TOP).round();
        if self.geometry != Some((pos, size, radius)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            let ppp = ctx.pixels_per_point();
            self.glass.shape(
                (size.x * ppp).round() as i32,
                (size.y * ppp).round() as i32,
                (radius * ppp).round() as i32,
            );
            self.geometry = Some((pos, size, radius));
        }
    }

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
                self.usage = rings::Usage::load(dir);
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

        // Sessions that need attention first, inactive ones last; most
        // recent first within each.
        let inactive_secs = self.settings.inactive_secs();
        // Taken out while drawing so the rows can borrow it alongside `self`.
        let sessions = std::mem::take(&mut self.sessions);
        let mut rows: Vec<(&Session, &str)> = sessions
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

        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        let full = ui.max_rect();
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

        let (state, line) = island::headline(&rows);
        if !self.expanded {
            let size = self.draw_pill(ui, full.min, &rows, state, &line, t);
            self.place_window(&ctx, size, PILL_H / 2.0);
            self.schedule_repaint(&ctx, FRAME_COLLAPSED);
        } else {
            let usage = self.usage.map(|u| {
                let (left, time) = u.fractions(now);
                (self.refill.value(left, t, self.settings.animate), time)
            });
            let size = self.draw_open(ui, full.min, &rows, state, usage, now, t);
            self.place_window(&ctx, size, RADIUS);
            self.schedule_repaint(&ctx, FRAME_EXPANDED);
        }
        drop(rows);
        self.sessions = sessions;
    }
}

impl App {
    /// The collapsed pill. Returns its size.
    fn draw_pill(
        &self,
        ui: &egui::Ui,
        origin: Pos2,
        rows: &[(&Session, &str)],
        state: &str,
        line: &Line,
        t: f64,
    ) -> Vec2 {
        let painter = ui.painter();
        let galley = painter.layout_job(text_job(line, FontId::proportional(15.0), PILL_TEXT_MAX));
        let dots = rows.len().min(MAX_DOTS);
        let dots_w = if dots == 0 {
            0.0
        } else {
            10.0 + dots as f32 * DOT + (dots - 1) as f32 * DOT_GAP
        };
        let width = 8.0 + PILL_OCTOPUS + 9.0 + galley.size().x + dots_w + 17.0;
        let rect = Rect::from_min_size(origin, vec2(width, PILL_H));
        paint_glass(painter, rect, PILL_H / 2.0, self.glass.blur);

        let mid = rect.center().y;
        let mascot = Rect::from_min_size(
            pos2(rect.left() + 8.0, mid - PILL_OCTOPUS / 2.0),
            Vec2::splat(PILL_OCTOPUS),
        );
        octopus::paint(painter, mascot, state, t);
        let text_x = mascot.right() + 9.0;
        let text_w = galley.size().x;
        painter.galley(
            pos2(text_x, mid - galley.size().y / 2.0),
            galley,
            Color32::WHITE,
        );
        let mut x = text_x + text_w + 10.0 + DOT / 2.0;
        for (_, st) in rows.iter().take(dots) {
            let p = pos2(x, mid);
            painter.circle_filled(p, DOT / 2.0 + 1.0, Color32::from_white_alpha(60));
            painter.circle_filled(p, DOT / 2.0, style::look(st).color);
            x += DOT + DOT_GAP;
        }
        rect.size()
    }

    /// The open island: rings and chips on top, details below. Returns its size.
    #[allow(clippy::too_many_arguments)]
    fn draw_open(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        rows: &[(&Session, &str)],
        state: &str,
        usage: Option<(f32, f32)>,
        now: u64,
        t: f64,
    ) -> Vec2 {
        let cols = rows.len().clamp(1, COLUMNS);
        let lines = rows.len().div_ceil(COLUMNS).max(1);
        let rings_w = if usage.is_some() { RINGS + PAD } else { 0.0 };
        let width =
            (2.0 * PAD + rings_w + cols as f32 * CHIP_W + (cols - 1) as f32 * CHIP_GAP).max(MIN_W);
        let chips_h = lines as f32 * CHIP_H + (lines - 1) as f32 * CHIP_GAP;
        let top_h = chips_h.max(if usage.is_some() { RINGS } else { 0.0 });
        let with_detail = !rows.is_empty();
        let sep_y = PAD + top_h + 10.0;
        let height = if with_detail {
            sep_y + 10.0 + self.detail_h + PAD
        } else {
            2.0 * PAD + top_h
        };
        let rect = Rect::from_min_size(origin, vec2(width, height));
        paint_glass(ui.painter(), rect, RADIUS, self.glass.blur);

        let row_top = rect.top() + PAD;
        let mut x = rect.left() + PAD;
        if let Some((left, time)) = usage {
            let ring =
                Rect::from_min_size(pos2(x, row_top + (top_h - RINGS) / 2.0), Vec2::splat(RINGS));
            rings::paint(ui.painter(), ring, left, time, state, t);
            x += rings_w;
        }

        if rows.is_empty() {
            let galley = ui.painter().layout_job(text_job(
                &island::summary(rows),
                FontId::proportional(15.0),
                width,
            ));
            let y = row_top + (top_h.max(galley.size().y) - galley.size().y) / 2.0;
            let x = if usage.is_some() {
                x
            } else {
                rect.center().x - galley.size().x / 2.0
            };
            ui.painter().galley(pos2(x, y), galley, Color32::WHITE);
            return rect.size();
        }

        let chip_w = (rect.right() - PAD - x - (cols - 1) as f32 * CHIP_GAP) / cols as f32;
        let chips_top = row_top + (top_h - chips_h) / 2.0;
        let mut hovered = self.hovered.clone();
        for (i, (s, st)) in rows.iter().enumerate() {
            let (col, line) = (i % COLUMNS, i / COLUMNS);
            let chip = Rect::from_min_size(
                pos2(
                    x + col as f32 * (chip_w + CHIP_GAP),
                    chips_top + line as f32 * (CHIP_H + CHIP_GAP),
                ),
                vec2(chip_w, CHIP_H),
            );
            if ui
                .interact(chip, ui.id().with(("chip", &s.id)), Sense::hover())
                .hovered()
            {
                hovered = Some(s.id.clone());
            }
            let on = hovered.as_deref() == Some(s.id.as_str());
            draw_chip(ui.painter(), chip, s, st, on, t);
        }
        self.hovered = hovered;

        ui.painter().hline(
            (rect.left() + PAD + 4.0)..=(rect.right() - PAD - 4.0),
            rect.top() + sep_y,
            Stroke::new(1.0, Color32::from_white_alpha(40)),
        );
        let area = Rect::from_min_max(
            pos2(rect.left() + PAD + 6.0, rect.top() + sep_y + 10.0),
            pos2(rect.right() - PAD - 6.0, rect.bottom() + 400.0),
        );
        let focus = self
            .hovered
            .as_ref()
            .and_then(|id| rows.iter().find(|(s, _)| &s.id == id));
        let detail = ui.scope_builder(UiBuilder::new().max_rect(area), |ui| match focus {
            Some((s, st)) => details(ui, s, st, now),
            None => {
                ui.label(text_job(
                    &island::summary(rows),
                    FontId::proportional(15.0),
                    area.width(),
                ));
            }
        });
        self.detail_h = detail.response.rect.height();
        rect.size()
    }
}

/// The glass: a translucent tint, lighter at the top, a thin bright edge and
/// a highlight along the top like the edge of a pane.
fn paint_glass(painter: &Painter, rect: Rect, radius: f32, blur: bool) {
    let tint = if blur {
        Color32::from_rgba_unmultiplied(26, 26, 38, 110)
    } else {
        Color32::from_rgba_unmultiplied(22, 22, 30, 235)
    };
    let outline = rounded_rect(rect, radius);
    let shade = |y: f32| {
        let k = ((y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let white = Color32::from_white_alpha((31.0 - 23.0 * k) as u8);
        tint.blend(white)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), shade(rect.center().y));
    for p in &outline {
        mesh.colored_vertex(*p, shade(p.y));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(mesh);
    painter.rect_stroke(
        rect,
        CornerRadius::same(radius.round() as u8),
        Stroke::new(1.0, Color32::from_white_alpha(56)),
        StrokeKind::Inside,
    );
    painter.hline(
        (rect.left() + radius)..=(rect.right() - radius),
        rect.top() + 1.5,
        Stroke::new(1.0, Color32::from_white_alpha(72)),
    );
}

/// The outline of a rounded rectangle, clockwise.
fn rounded_rect(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    let steps = 10;
    let mut points = Vec::with_capacity(4 * (steps + 1));
    for (c, start) in corners {
        for i in 0..=steps {
            let a = (start + 90.0 * i as f32 / steps as f32).to_radians();
            points.push(c + r * vec2(a.cos(), a.sin()));
        }
    }
    points
}

/// One session: its octopus, name, state and task progress.
fn draw_chip(painter: &Painter, rect: Rect, s: &Session, state: &str, on: bool, t: f64) {
    let (fill, edge) = if on { (51, 89) } else { (20, 26) };
    painter.rect(
        rect,
        CornerRadius::same(15),
        Color32::from_white_alpha(fill),
        Stroke::new(1.0, Color32::from_white_alpha(edge)),
        StrokeKind::Inside,
    );
    let look = style::look(state);
    let inner = rect.shrink2(vec2(10.0, 8.0));
    let mascot = Rect::from_min_size(inner.min, Vec2::splat(CHIP_OCTOPUS));
    octopus::paint(painter, mascot, state, t);

    let text_x = mascot.right() + 8.0;
    let text_w = (inner.right() - text_x).max(10.0);
    let name = painter.layout_job(one_line(
        &s.project,
        fonts::semibold(14.5),
        Color32::WHITE,
        text_w,
    ));
    let label = painter.layout_job(one_line(
        sessions::label(state),
        fonts::semibold(12.0),
        look.color,
        text_w,
    ));
    let block = name.size().y + label.size().y;
    let y = mascot.center().y - block / 2.0;
    let name_h = name.size().y;
    painter.galley(pos2(text_x, y), name, Color32::WHITE);
    painter.galley(pos2(text_x, y + name_h), label, look.color);

    let bar = Rect::from_min_max(
        pos2(inner.left(), inner.bottom() - 5.0),
        pos2(inner.right(), inner.bottom()),
    );
    painter.rect_filled(bar, CornerRadius::same(3), Color32::from_white_alpha(46));
    let (done, total) = s.task_counts();
    let progress = if total > 0 {
        done as f32 / total as f32
    } else if state == "done" {
        1.0
    } else {
        0.0
    };
    if progress > 0.0 {
        let mut filled = bar;
        filled.set_width((bar.width() * progress).max(bar.height()));
        painter.rect_filled(filled, CornerRadius::same(3), look.color);
    }
}

/// Details of the chip under the mouse: what it is doing now, then its
/// tasks as steps, what it asks permission for, or the original request.
fn details(ui: &mut egui::Ui, s: &Session, state: &str, now: u64) {
    let look = style::look(state);
    let waiting = state == "waiting";
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 2.0);
        ui.vertical(|ui| {
            ui.set_max_width(250.0);
            caption(
                ui,
                if waiting {
                    "PIDE PERMISO PARA"
                } else {
                    "AHORA"
                },
            );
            let now_text = match (waiting, s.detail.as_deref()) {
                (true, Some(d)) => d.to_string(),
                (true, None) => "Continuar".to_string(),
                (false, Some(d)) => format!("{} · {d}", sessions::label(state)),
                (false, None) => sessions::label(state).to_string(),
            };
            ui.add(egui::Label::new(RichText::new(now_text).size(15.0)).truncate());
            ui.label(
                RichText::new(island::ago(now, s.ts))
                    .size(11.5)
                    .color(Color32::from_white_alpha(200)),
            );
        });
        ui.add_space(16.0);

        if waiting {
            egui::Frame::new()
                .fill(look.color.gamma_multiply(0.2))
                .stroke(Stroke::new(1.0, look.color.gamma_multiply(0.55)))
                .corner_radius(12)
                .inner_margin(vec2(12.0, 6.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Respóndele en su terminal").size(13.5));
                });
        } else if !s.tasks.is_empty() {
            steps(ui, s);
        } else if let Some(prompt) = &s.prompt {
            ui.vertical(|ui| {
                caption(ui, "PEDIDO");
                let mut job = LayoutJob::single_section(
                    prompt.clone(),
                    TextFormat::simple(FontId::proportional(13.5), Color32::from_white_alpha(230)),
                );
                job.wrap.max_width = ui.available_width();
                job.wrap.max_rows = 2;
                ui.label(job);
            });
        }
    });
}

/// The task list as a row of steps: done, the current one highlighted, and
/// what's left.
fn steps(ui: &mut egui::Ui, s: &Session) {
    let (start, end, after) = island::task_window(s, MAX_STEPS);
    let width = ui.available_width();
    ui.allocate_ui_with_layout(
        vec2(width, 0.0),
        Layout::left_to_right(Align::Center).with_main_wrap(true),
        |ui| {
            ui.spacing_mut().item_spacing = vec2(5.0, 5.0);
            let dim = Color32::from_white_alpha(130);
            if start > 0 {
                ui.label(
                    RichText::new(format!("{start} hechas ›"))
                        .size(12.5)
                        .color(dim),
                );
            }
            for (i, task) in s.tasks[start..end].iter().enumerate() {
                if i > 0 {
                    ui.label(RichText::new("›").size(12.5).color(dim));
                }
                let (mark, fill, alpha, font) = match task.status.as_str() {
                    "completed" => ("✔ ", 20, 180, FontId::proportional(12.5)),
                    "in_progress" => ("▶ ", 72, 255, fonts::semibold(12.5)),
                    _ => ("", 26, 235, FontId::proportional(12.5)),
                };
                let text = RichText::new(format!("{mark}{}", clip(&task.text, 30)))
                    .font(font)
                    .color(Color32::from_white_alpha(alpha));
                egui::Frame::new()
                    .fill(Color32::from_white_alpha(fill))
                    .corner_radius(10)
                    .inner_margin(vec2(9.0, 3.0))
                    .show(ui, |ui| {
                        ui.label(text);
                    });
            }
            if after > 0 {
                ui.label(
                    RichText::new(format!("› {after} más"))
                        .size(12.5)
                        .color(dim),
                );
            }
        },
    );
}

fn caption(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(fonts::semibold(10.5))
            .color(Color32::from_white_alpha(190))
            .extra_letter_spacing(1.2),
    );
}

/// Island text with its tones, on one line of at most `max_width`.
fn text_job(line: &Line, font: FontId, max_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (text, tone) in line {
        let (color, font) = match tone {
            Tone::Normal => (Color32::WHITE, font.clone()),
            Tone::Dim => (Color32::from_white_alpha(170), font.clone()),
            Tone::State(st) => (style::look(st).color, fonts::semibold(font.size)),
        };
        job.append(text, 0.0, TextFormat::simple(font, color));
    }
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job
}

fn one_line(text: &str, font: FontId, color: Color32, max_width: f32) -> LayoutJob {
    text_job(&vec![(text.to_string(), Tone::Normal)], font, max_width).recolored(color)
}

trait Recolor {
    fn recolored(self, color: Color32) -> Self;
}

impl Recolor for LayoutJob {
    fn recolored(mut self, color: Color32) -> Self {
        for s in &mut self.sections {
            s.format.color = color;
        }
        self
    }
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max - 1).collect();
    format!("{}…", cut.trim_end())
}
