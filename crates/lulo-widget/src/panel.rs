//! The detail panel under the chips: what one session is working on, with
//! what model and permissions, what it costs the machine, what was denied
//! or asked, and its task list. Opens with a click on the session's chip.
//! Every part is drawn only when there is something to say.

use std::sync::Arc;

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect,
    Sense, Shape, Stroke, StrokeKind, TextFormat,
};

use crate::fonts;
use crate::procinfo::{self, Usage};
use crate::sessions::{self, Session};
use crate::style;

const FILL: Color32 = Color32::from_rgb(40, 40, 52);
const MUTED: Color32 = Color32::from_rgb(154, 154, 174);
const PAD: egui::Vec2 = vec2(12.0, 10.0);
const COL_GAP: f32 = 14.0;
const ITEM_GAP: f32 = 7.0;
/// Between two tasks, so the list reads as one block.
const TASK_GAP: f32 = 2.0;
const NOTCH: f32 = 6.0;
/// Tasks listed before "y N más".
const MAX_TASKS: usize = 8;
const MARK_W: f32 = 14.0;

enum Item {
    Text(Arc<Galley>),
    Pills(Vec<(Arc<Galley>, Color32)>),
    Alert {
        title: Arc<Galley>,
        body: Arc<Galley>,
        tint: Color32,
    },
    Task(Arc<Galley>, Mark),
    Button(Arc<Galley>),
}

#[derive(Clone, Copy)]
enum Mark {
    Done,
    Now,
    Todo,
}

const PILL_PAD: egui::Vec2 = vec2(7.0, 2.5);
const PILL_GAP: f32 = 4.0;

impl Item {
    fn height(&self, width: f32) -> f32 {
        match self {
            Item::Text(g) | Item::Task(g, _) => g.size().y,
            Item::Pills(pills) => pill_rows(pills, width)
                .last()
                .map_or(0.0, |(y, _)| y + pill_h(pills)),
            Item::Alert { title, body, .. } => title.size().y + body.size().y + 10.0,
            Item::Button(g) => g.size().y + 10.0,
        }
    }
}

fn pill_h(pills: &[(Arc<Galley>, Color32)]) -> f32 {
    pills
        .first()
        .map_or(0.0, |(g, _)| g.size().y + 2.0 * PILL_PAD.y)
}

/// Top-left offset of every pill, wrapping to a new row when one doesn't fit.
fn pill_rows(pills: &[(Arc<Galley>, Color32)], width: f32) -> Vec<(f32, f32)> {
    let h = pill_h(pills);
    let (mut x, mut y) = (0.0, 0.0);
    pills
        .iter()
        .map(|(g, _)| {
            let w = g.size().x + 2.0 * PILL_PAD.x;
            if x > 0.0 && x + w > width {
                x = 0.0;
                y += h + PILL_GAP;
            }
            let at = (y, x);
            x += w + PILL_GAP;
            at
        })
        .collect()
}

fn text(
    painter: &Painter,
    s: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(s.to_string(), TextFormat::simple(font, color));
    job.wrap.max_width = width;
    job.wrap.max_rows = rows;
    job.wrap.break_anywhere = rows == 1;
    painter.layout_job(job)
}

fn gap(a: &Item, b: &Item) -> f32 {
    match (a, b) {
        (Item::Task(..), Item::Task(..)) => TASK_GAP,
        _ => ITEM_GAP,
    }
}

/// Draws the panel with its top-left at `at`, pointing up at `notch_x`.
/// Returns its bottom edge and whether "Ir a la sesión" was clicked.
pub fn draw(
    ui: &mut egui::Ui,
    at: Pos2,
    width: f32,
    notch_x: f32,
    s: &Session,
    usage: Option<Usage>,
    now: u64,
) -> (f32, bool) {
    let painter = ui.painter().clone();
    let col_w = (width - 2.0 * PAD.x - COL_GAP) / 2.0;
    let info = &s.info;

    let mut left = Vec::new();
    if let Some(title) = &info.title {
        left.push(Item::Text(text(
            &painter,
            title,
            fonts::semibold(12.5),
            Color32::WHITE,
            col_w,
            2,
        )));
    }
    let mut pills = Vec::new();
    let mut pill = |label: String, color: Color32| {
        let g = text(
            &painter,
            &label,
            FontId::proportional(10.5),
            color,
            col_w,
            1,
        );
        pills.push((g, color));
    };
    if let Some(m) = &info.model {
        pill(sessions::model_label(m), style::TEXT);
    }
    if let Some(e) = &info.effort {
        pill(sessions::effort_label(e), style::TEXT);
    }
    if let Some(mode) = &info.permission_mode {
        let color = match mode.as_str() {
            "auto" => Color32::from_rgb(196, 181, 253),
            "bypassPermissions" => style::look("error").color,
            _ => style::TEXT,
        };
        pill(sessions::mode_label(mode), color);
    }
    if !pills.is_empty() {
        left.push(Item::Pills(pills));
    }
    if let Some(u) = usage {
        let mut job = LayoutJob::default();
        let part = |job: &mut LayoutJob, t: &str, value: bool| {
            let font = if value {
                fonts::semibold(10.5)
            } else {
                FontId::proportional(10.5)
            };
            job.append(
                t,
                0.0,
                TextFormat::simple(font, if value { Color32::WHITE } else { MUTED }),
            );
        };
        part(&mut job, "CPU ", false);
        let cpu = u
            .cpu
            .map_or("…".to_string(), |c| format!("{}%", c.round() as u32));
        part(&mut job, &cpu, true);
        part(&mut job, "     RAM ", false);
        part(&mut job, &procinfo::bytes(u.ram), true);
        left.push(Item::Text(painter.layout_job(job)));
    }
    left.push(Item::Button(text(
        &painter,
        "Ir a la sesión",
        fonts::semibold(10.5),
        Color32::from_rgb(15, 15, 21),
        col_w,
        1,
    )));

    let mut right = Vec::new();
    let alert_w = col_w - 14.0;
    if let Some((tool, detail, ts)) = &info.denied {
        let when = match now.saturating_sub(*ts) {
            0..60 => "justo ahora".to_string(),
            secs => format!("hace {}", sessions::duration(secs)),
        };
        let what = match detail {
            Some(d) => format!("{tool}: {d} · {when}"),
            None => format!("{tool} · {when}"),
        };
        let tint = style::look("error").color;
        right.push(Item::Alert {
            title: text(
                &painter,
                "Negado por el modo auto",
                fonts::semibold(10.5),
                tint,
                alert_w,
                1,
            ),
            body: text(
                &painter,
                &what,
                FontId::proportional(10.5),
                style::TEXT,
                alert_w,
                2,
            ),
            tint,
        });
    }
    if let Some((server, question)) = &info.question {
        let tint = style::look("waiting").color;
        let ask = question
            .as_deref()
            .unwrap_or("Necesita una respuesta en la sesión");
        right.push(Item::Alert {
            title: text(
                &painter,
                &format!("{server} te pregunta"),
                fonts::semibold(10.5),
                tint,
                alert_w,
                1,
            ),
            body: text(
                &painter,
                ask,
                FontId::proportional(10.5),
                style::TEXT,
                alert_w,
                3,
            ),
            tint,
        });
    }
    let (done, total) = s.task_counts();
    if total > 0 {
        let head = format!("TAREAS · {done} DE {total}");
        right.push(Item::Text(text(
            &painter,
            &head,
            fonts::semibold(9.5),
            MUTED,
            col_w,
            1,
        )));
        for task in s.tasks.iter().take(MAX_TASKS) {
            let mark = match task.status.as_str() {
                "completed" => Mark::Done,
                "in_progress" => Mark::Now,
                _ => Mark::Todo,
            };
            let color = if matches!(mark, Mark::Done) {
                MUTED
            } else {
                style::TEXT
            };
            let mut format = TextFormat::simple(FontId::proportional(11.0), color);
            if matches!(mark, Mark::Done) {
                format.strikethrough = Stroke::new(1.0, Color32::from_white_alpha(70));
            }
            let mut job = LayoutJob::single_section(task.text.clone(), format);
            job.wrap.max_width = col_w - MARK_W;
            job.wrap.max_rows = 2;
            right.push(Item::Task(painter.layout_job(job), mark));
        }
        if total > MAX_TASKS {
            let more = format!("y {} más", total - MAX_TASKS);
            right.push(Item::Text(text(
                &painter,
                &more,
                FontId::proportional(10.5),
                MUTED,
                col_w,
                1,
            )));
        }
    } else if right.is_empty() {
        right.push(Item::Text(text(
            &painter,
            "Sin lista de tareas.",
            FontId::proportional(10.5),
            MUTED,
            col_w,
            1,
        )));
    }

    let column_h = |items: &[Item]| {
        let gaps: f32 = items.windows(2).map(|w| gap(&w[0], &w[1])).sum();
        items.iter().map(|i| i.height(col_w)).sum::<f32>() + gaps
    };
    let height = 2.0 * PAD.y + column_h(&left).max(column_h(&right));
    let rect = Rect::from_min_size(at, vec2(width, height));

    // Panel, then a notch pointing at the chip it belongs to.
    let edge = Stroke::new(1.0, Color32::from_white_alpha(40));
    painter.rect(rect, CornerRadius::same(14), FILL, edge, StrokeKind::Inside);
    let nx = notch_x.clamp(rect.left() + 20.0, rect.right() - 20.0);
    let tip = pos2(nx, rect.top() - NOTCH);
    let (a, b) = (
        pos2(nx - NOTCH, rect.top() + 0.5),
        pos2(nx + NOTCH, rect.top() + 0.5),
    );
    painter.add(Shape::convex_polygon(vec![a, tip, b], FILL, Stroke::NONE));
    painter.line_segment([pos2(a.x, rect.top()), tip], edge);
    painter.line_segment([tip, pos2(b.x, rect.top())], edge);

    let mut go = false;
    let mut place = |items: Vec<Item>, x: f32, ui: &mut egui::Ui| {
        let mut y = rect.top() + PAD.y;
        for (i, item) in items.iter().enumerate() {
            let h = item.height(col_w);
            match item {
                Item::Text(g) => painter.galley(pos2(x, y), g.clone(), Color32::WHITE),
                Item::Pills(pills) => {
                    let ph = pill_h(pills);
                    for ((g, color), (py, px)) in pills.iter().zip(pill_rows(pills, col_w)) {
                        let r = Rect::from_min_size(
                            pos2(x + px, y + py),
                            vec2(g.size().x + 2.0 * PILL_PAD.x, ph),
                        );
                        let border = if *color == style::TEXT {
                            Color32::from_white_alpha(26)
                        } else {
                            color.gamma_multiply(0.45)
                        };
                        painter.rect(
                            r,
                            CornerRadius::same(9),
                            Color32::from_white_alpha(12),
                            Stroke::new(1.0, border),
                            StrokeKind::Inside,
                        );
                        painter.galley(r.min + PILL_PAD, g.clone(), *color);
                    }
                }
                Item::Alert { title, body, tint } => {
                    let r = Rect::from_min_size(pos2(x, y), vec2(col_w, h));
                    painter.rect(
                        r,
                        CornerRadius::same(8),
                        tint.gamma_multiply(0.14),
                        Stroke::new(1.0, tint.gamma_multiply(0.4)),
                        StrokeKind::Inside,
                    );
                    painter.galley(pos2(x + 7.0, y + 5.0), title.clone(), *tint);
                    painter.galley(
                        pos2(x + 7.0, y + 5.0 + title.size().y),
                        body.clone(),
                        style::TEXT,
                    );
                }
                Item::Task(g, mark) => {
                    let c = pos2(x + 4.5, y + 7.5);
                    match mark {
                        Mark::Done => {
                            let ok = style::look("done").color;
                            painter.line_segment(
                                [c + vec2(-3.5, 0.0), c + vec2(-1.0, 2.5)],
                                Stroke::new(1.4, ok),
                            );
                            painter.line_segment(
                                [c + vec2(-1.0, 2.5), c + vec2(3.5, -2.5)],
                                Stroke::new(1.4, ok),
                            );
                        }
                        Mark::Now => {
                            let now = style::look("editing").color;
                            painter.add(Shape::convex_polygon(
                                vec![
                                    c + vec2(-2.5, -3.5),
                                    c + vec2(3.0, 0.0),
                                    c + vec2(-2.5, 3.5),
                                ],
                                now,
                                Stroke::NONE,
                            ));
                        }
                        Mark::Todo => {
                            painter.circle_stroke(
                                c,
                                3.2,
                                Stroke::new(1.1, style::look("inactive").color),
                            );
                        }
                    }
                    painter.galley(pos2(x + MARK_W, y), g.clone(), style::TEXT);
                }
                Item::Button(g) => {
                    let r = Rect::from_min_size(pos2(x, y), g.size() + vec2(20.0, 10.0));
                    let response = ui.interact(r, ui.id().with("panel-go"), Sense::click());
                    let fill = if response.hovered() {
                        Color32::WHITE
                    } else {
                        style::TEXT
                    };
                    painter.rect_filled(r, CornerRadius::same(10), fill);
                    painter.galley(r.min + vec2(10.0, 5.0), g.clone(), Color32::BLACK);
                    go |= response.clicked();
                }
            }
            y += h + items.get(i + 1).map_or(0.0, |next| gap(item, next));
        }
    };
    place(left, rect.left() + PAD.x, ui);
    place(right, rect.left() + PAD.x + col_w + COL_GAP, ui);
    (rect.bottom(), go)
}
