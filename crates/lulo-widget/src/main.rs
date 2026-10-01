//! Lulo: a small dark half moon hanging from the top edge of the screen
//! that shows every Claude Code session and what it is doing right now.
//!
//! Collapsed, the octopus peeks out from under the moon upside down, its
//! eyes acting out what most needs you, with one colored dot per active
//! session. Hovering it drops one chip per session under the moon, with
//! nothing behind them.
//!
//! It watches the folder `lulo-hook` writes to. Without animations it only
//! redraws when a file changes or the mouse is over it (plus a slow tick);
//! with them, the octopus runs at a low frame rate while collapsed and a
//! smooth one while open.

// No console window behind the widget on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod alert;
mod autostart;
mod changes;
mod cursor;
mod display;
mod focus;
mod fonts;
mod glass;
mod island;
mod mood;
mod octopus;
mod sessions;
mod settings;
mod style;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense,
    Stroke, StrokeKind, TextFormat, Vec2,
};
use notify::{RecursiveMode, Watcher};

use display::Anchor;
use octopus::Act;
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

// Half moon: its width, the free space on each side of the dots, and dots.
const MOON_W: f32 = 96.0;
const MOON_SIDE: f32 = 22.0;
const DOT_R: f32 = 2.4;
const DOT_STEP: f32 = 8.0;
const MAX_DOTS: usize = 10;

// Open: the chips hang just under the moon.
const ROW_GAP: f32 = 2.0;
const COLUMNS: usize = 4;
const CHIP_W: f32 = 156.0;
const CHIP_H: f32 = 50.0;
const GAP: f32 = 6.0;
const CHIP_OCTOPUS: f32 = 30.0;
/// How close the mouse must be for the octopus to look at it (points).
const NEAR_MOUSE: f32 = 260.0;
/// Height of the eyes on the moon's canvas.
const MOON_EYES_Y: f32 = 35.5;
/// Window size needed while the right-click menu is open.
const MENU_ROOM: Vec2 = vec2(360.0, 230.0);
/// How often to check for something full screen.
const FULLSCREEN_POLL: Duration = Duration::from_secs(2);
/// Solid backgrounds for the chips.
const SOLID: Color32 = Color32::from_rgb(23, 23, 30);
const SOLID_HOVER: Color32 = Color32::from_rgb(40, 40, 52);
/// The time next to the state on a chip.
const MUTED: Color32 = Color32::from_rgb(154, 154, 174);

fn main() -> eframe::Result {
    let settings = Settings::load();
    // Writes the defaults on first run so the settings are there to edit.
    settings.save();
    let viewport = egui::ViewportBuilder::default()
        .with_title("Lulo")
        .with_inner_size([MOON_W, 60.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        // Out of the taskbar and Alt+Tab, and don't steal focus on launch.
        .with_taskbar(false)
        .with_active(false)
        .with_resizable(false)
        .with_icon(egui::IconData {
            rgba: include_bytes!("../../../assets/lulo-64.rgba").to_vec(),
            width: 64,
            height: 64,
        });
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
    expanded: bool,
    last_inside: Option<Instant>,
    /// Session whose details are shown; stays while the mouse moves down to them.
    hovered: Option<String>,
    geometry: Option<(Pos2, Vec2)>,
    mood: mood::Mood,
    watch: changes::Watch,
    alerts: alert::Alerts,
    /// Monitors, re-read with the session files.
    monitors: Vec<display::Monitor>,
    /// Something full screen has the screen, and when that was checked.
    fullscreen: bool,
    fullscreen_at: Option<Instant>,
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
        glass::apply(cc);
        alert::register();
        App {
            dir,
            sessions: Vec::new(),
            dirty,
            _watcher: watcher,
            last_load: None,
            settings,
            autostart: autostart::is_enabled(),
            expanded: false,
            last_inside: None,
            hovered: None,
            geometry: None,
            mood: mood::Mood::default(),
            watch: changes::Watch::default(),
            alerts: alert::Alerts::start(),
            monitors: display::monitors(),
            fullscreen: false,
            fullscreen_at: None,
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

    /// Sticks the window to the top edge of the chosen monitor, at the
    /// chosen side.
    fn place_window(&mut self, ctx: &egui::Context, size: Vec2) {
        let size = size.round();
        let s = &self.settings;
        let pos = match display::pick(&self.monitors, Some(s.monitor)) {
            Some(m) => {
                let size_px = (size.x * m.scale, size.y * m.scale);
                let (x, y) = display::place(&self.monitors, Some(s.monitor), s.anchor, size_px)
                    .unwrap_or_default();
                // egui turns points into pixels with the scale of the
                // monitor the window is on now; once it lands on another,
                // the next frame places it again with that one's scale.
                let ppp = ctx.pixels_per_point();
                pos2(x / ppp, y / ppp).round()
            }
            // No monitor list off Windows: center on the current screen.
            None => {
                let screen = ctx
                    .input(|i| i.viewport().monitor_size)
                    .unwrap_or(vec2(1920.0, 1080.0));
                pos2(((screen.x - size.x) / 2.0).max(0.0), 0.0).round()
            }
        };
        if self.geometry != Some((pos, size)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            self.geometry = Some((pos, size));
        }
    }

    /// Toasts and sounds for the sessions that just changed.
    fn alert(&mut self, now: u64) {
        let inactive_secs = self.settings.inactive_secs();
        let rows: Vec<(&Session, &str)> = self
            .sessions
            .iter()
            .map(|s| (s, s.shown_state(now, inactive_secs)))
            .collect();
        let events = self.watch.update(&rows);
        let s = &self.settings;
        let options = alert::Options {
            toasts: s.notifications,
            sounds: s.sounds,
            new_session_sound: s.new_session_sound,
        };
        self.alerts.send(events, options);
    }

    /// The right-click menu: autostart, alerts and close.
    fn menu(&mut self, ui: &mut egui::Ui) {
        if autostart::SUPPORTED
            && ui
                .checkbox(&mut self.autostart, "Iniciar con Windows")
                .changed()
        {
            autostart::set(self.autostart);
            self.autostart = autostart::is_enabled();
        }
        let s = &mut self.settings;
        let mut changed = ui
            .checkbox(&mut s.notifications, "Avisos de Windows")
            .changed();
        changed |= ui.checkbox(&mut s.sounds, "Sonidos").changed();
        changed |= ui
            .checkbox(&mut s.hide_fullscreen, "Esconder en pantalla completa")
            .changed();
        if self.monitors.len() > 1 {
            ui.menu_button("Pantalla", |ui| {
                for i in 0..self.monitors.len() {
                    let name = if self.monitors[i].primary {
                        format!("Pantalla {} (principal)", i + 1)
                    } else {
                        format!("Pantalla {}", i + 1)
                    };
                    changed |= ui.radio_value(&mut s.monitor, i, name).changed();
                }
            });
        }
        ui.menu_button("Posición", |ui| {
            for (anchor, name) in [
                (Anchor::Left, "Izquierda"),
                (Anchor::Center, "Centro"),
                (Anchor::Right, "Derecha"),
            ] {
                changed |= ui.radio_value(&mut s.anchor, anchor, name).changed();
            }
        });
        if changed {
            s.save();
        }
        if ui.button("Cerrar Lulo").clicked() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Next frame for the animation, or just the slow tick when it is off.
    fn schedule_repaint(&self, ctx: &egui::Context, frame: Duration) {
        let idle = if self.settings.hide_fullscreen {
            FULLSCREEN_POLL
        } else {
            TICK
        };
        ctx.request_repaint_after(if self.settings.animate { frame } else { idle });
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
                self.alert(now);
            }
            self.monitors = display::monitors();
            self.last_load = Some(Instant::now());
        }
        if self.settings.hide_fullscreen
            && self
                .fullscreen_at
                .is_none_or(|t| t.elapsed() >= FULLSCREEN_POLL)
        {
            self.fullscreen = display::busy_fullscreen();
            self.fullscreen_at = Some(Instant::now());
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

        // Out of the way of a video, game or presentation, unless a session
        // needs you. The window stays (a hidden one stops getting frames, and
        // this check with it), shrunk to one transparent pixel.
        let needs_you = rows
            .iter()
            .any(|(_, st)| matches!(*st, "waiting" | "error"));
        if self.settings.hide_fullscreen && self.fullscreen && !needs_you {
            self.expanded = false;
            self.place_window(&ctx, vec2(1.0, 1.0));
            ctx.request_repaint_after(FULLSCREEN_POLL);
            drop(rows);
            self.sessions = sessions;
            return;
        }

        ui.visuals_mut().override_text_color = Some(Color32::WHITE);
        let full = ui.max_rect();
        // Right-click anywhere: autostart and close.
        let background = ui.interact(full, ui.id().with("bg"), Sense::click());
        background.context_menu(|ui| self.menu(ui));

        let state = island::focus(&rows);
        let active: Vec<&str> = rows
            .iter()
            .map(|(_, st)| *st)
            .filter(|st| *st != "inactive")
            .collect();
        let open = self.expanded && !rows.is_empty();
        let moon = moon_size(active.len());
        let mut size = moon;

        // Short reactions of the octopus on top of its state.
        let act = if self.settings.animate {
            let now = ctx.input(|i| i.time);
            self.mood
                .observe(rows.iter().map(|(s, st)| (s.id.as_str(), *st)), state, now);
            let eyes = pos2(moon.x / 2.0, MOON_EYES_Y * moon.x / octopus::MOON_SIZE.x);
            let look = if open { None } else { nearby_mouse(&ctx, eyes) };
            self.mood.act(state, now, !open, look)
        } else {
            Act::None
        };

        if open {
            size = self.draw_open(ui, full.min, moon, &rows, t, now);
        } else {
            // The yo-yo hangs below the moon's usual canvas.
            size.y = moon.y * act.canvas_height() / octopus::MOON_SIZE.y;
        }
        // Clicks on the moon count toward annoying the octopus.
        let moon_rect =
            Rect::from_min_size(pos2(full.min.x + (size.x - moon.x) / 2.0, full.min.y), moon);
        let poke = ui.interact(moon_rect, ui.id().with("moon"), Sense::click());
        if poke.clicked() {
            self.mood.click(ctx.input(|i| i.time));
        }
        poke.context_menu(|ui| self.menu(ui));
        // Last, so the moon and the peeking head sit over everything else.
        draw_moon(
            ui.painter(),
            full.min.x + size.x / 2.0,
            full.min.y,
            moon,
            &active,
            state,
            t,
            act,
        );
        // Room for the right-click menu, which draws inside the window.
        if egui::Popup::is_any_open(&ctx) {
            size = size.max(MENU_ROOM);
        }
        self.place_window(&ctx, size);
        self.schedule_repaint(
            &ctx,
            if open {
                FRAME_EXPANDED
            } else {
                FRAME_COLLAPSED
            },
        );
        drop(rows);
        self.sessions = sessions;
    }
}

/// Direction from the octopus's eyes (`eyes`, in window points) to the mouse
/// when it is near the widget but outside it.
fn nearby_mouse(ctx: &egui::Context, eyes: Pos2) -> Option<Vec2> {
    let (x, y) = cursor::screen_px()?;
    let (outer, ppp) = ctx.input(|i| (i.viewport().outer_rect, i.pixels_per_point));
    let d = pos2(x / ppp, y / ppp) - (outer?.min + eyes.to_vec2());
    let dist = d.length();
    (dist > 1.0 && dist < NEAR_MOUSE).then(|| d / dist)
}

/// Size of the half moon: wider only when many dots need room.
fn moon_size(dots: usize) -> Vec2 {
    let row = dots.min(MAX_DOTS).saturating_sub(1) as f32 * DOT_STEP;
    let width = MOON_W.max(row + 2.0 * MOON_SIDE);
    vec2(width, width * octopus::MOON_SIZE.y / octopus::MOON_SIZE.x)
}

/// The half moon centered on `center_x`, with one dot per active session.
#[allow(clippy::too_many_arguments)]
fn draw_moon(
    painter: &Painter,
    center_x: f32,
    top: f32,
    size: Vec2,
    active: &[&str],
    state: &str,
    t: f64,
    act: Act,
) {
    let moon = Rect::from_min_size(pos2(center_x - size.x / 2.0, top), size);
    octopus::moon(painter, moon, state, t, act);
    let dots = active.len().min(MAX_DOTS);
    let y = top + octopus::MOON_DOTS_Y * size.x / octopus::MOON_SIZE.x;
    let mut x = center_x - dots.saturating_sub(1) as f32 * DOT_STEP / 2.0;
    for st in active.iter().take(dots) {
        painter.circle_filled(pos2(x, y), DOT_R + 0.8, Color32::from_white_alpha(40));
        painter.circle_filled(pos2(x, y), DOT_R, style::look(st).color);
        x += DOT_STEP;
    }
}

impl App {
    /// The open widget under the half moon: one chip per session, with
    /// nothing behind them. Returns the size of everything together, moon
    /// included.
    fn draw_open(
        &mut self,
        ui: &mut egui::Ui,
        origin: Pos2,
        moon: Vec2,
        rows: &[(&Session, &str)],
        t: f64,
        now: u64,
    ) -> Vec2 {
        let cols = rows.len().clamp(1, COLUMNS);
        let lines = rows.len().div_ceil(COLUMNS);
        let row_w = cols as f32 * CHIP_W + (cols - 1) as f32 * GAP;
        let row_h = lines as f32 * CHIP_H + lines.saturating_sub(1) as f32 * GAP;
        let width = row_w.max(moon.x);
        let top = origin.y + moon.y + ROW_GAP;
        let size = vec2(width, top - origin.y + row_h);

        let x = origin.x + (width - row_w) / 2.0;
        let mut hovered = self.hovered.clone();
        let mut forget = None;
        for (i, (s, st)) in rows.iter().enumerate() {
            let (col, line) = (i % COLUMNS, i / COLUMNS);
            let chip = Rect::from_min_size(
                pos2(
                    x + col as f32 * (CHIP_W + GAP),
                    top + line as f32 * (CHIP_H + GAP),
                ),
                vec2(CHIP_W, CHIP_H),
            );
            // A click brings the session's window to the front. The right
            // click opens the menu, which also offers to drop inactive ones.
            let response = ui.interact(chip, ui.id().with(("chip", &s.id)), Sense::click());
            if response.hovered() {
                hovered = Some(s.id.clone());
            }
            if response.clicked() {
                focus::bring_to_front(s.claude_pid);
            }
            response.context_menu(|ui| {
                if *st == "inactive" {
                    if ui.button("Quitar de la lista").clicked() {
                        forget = Some(s.id.clone());
                    }
                    ui.separator();
                }
                self.menu(ui);
            });
            let on = hovered.as_deref() == Some(s.id.as_str());
            draw_chip(ui.painter(), chip, s, st, on, t, now);
        }
        self.hovered = hovered;
        if let (Some(id), Some(dir)) = (forget, &self.dir) {
            sessions::forget(dir, &id);
            self.dirty.store(true, Ordering::Relaxed);
            ui.ctx().request_repaint();
        }
        size
    }
}

/// One session: its octopus, name, state and task progress.
fn draw_chip(painter: &Painter, rect: Rect, s: &Session, state: &str, on: bool, t: f64, now: u64) {
    painter.rect(
        rect,
        CornerRadius::same(14),
        if on { SOLID_HOVER } else { SOLID },
        Stroke::new(1.0, Color32::from_white_alpha(if on { 40 } else { 18 })),
        StrokeKind::Inside,
    );
    let look = style::look(state);
    let inner = rect.shrink2(vec2(9.0, 7.0));
    let mascot = Rect::from_min_size(inner.min, Vec2::splat(CHIP_OCTOPUS));
    octopus::paint(painter, mascot, state, t);

    let text_x = mascot.right() + 8.0;
    let text_w = (inner.right() - text_x).max(10.0);
    // How long it has been in this state, small at the top right; the
    // name gets what is left of the line.
    let time = s.elapsed(state, now).map(|secs| {
        painter.layout_no_wrap(sessions::duration(secs), FontId::proportional(10.5), MUTED)
    });
    let time_w = time.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let name = painter.layout_job(one_line(
        &s.project,
        fonts::semibold(13.5),
        Color32::WHITE,
        (text_w - time_w).max(10.0),
    ));
    let mut label = one_line(
        sessions::label(state),
        fonts::semibold(11.5),
        look.color,
        text_w,
    );
    // Work left running in the background while Claude does something else.
    if let Some(extra) = sessions::background_note(s, state) {
        label.append(
            &extra,
            0.0,
            TextFormat::simple(fonts::semibold(11.5), style::look("background").color),
        );
    }
    let label = painter.layout_job(label);
    let block = name.size().y + label.size().y;
    let y = mascot.center().y - block / 2.0;
    let name_h = name.size().y;
    if let Some(time) = time {
        // Sits on the name's baseline.
        let ty = y + name_h - time.size().y - 1.0;
        painter.galley(pos2(inner.right() - time.size().x, ty), time, MUTED);
    }
    painter.galley(pos2(text_x, y), name, Color32::WHITE);
    painter.galley(pos2(text_x, y + name_h), label, look.color);

    let bar = Rect::from_min_max(
        pos2(inner.left(), inner.bottom() - 4.0),
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

fn one_line(text: &str, font: FontId, color: Color32, max_width: f32) -> LayoutJob {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat::simple(font, color));
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job
}
