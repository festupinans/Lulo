//! Mirrors painted shapes top to bottom, so the half moon drawn for the top
//! edge also sits on the bottom edge: the moon turns into a dome and the
//! octopus, which hangs upside down from the top, peeks up from it.
//!
//! Text keeps reading the right way up; only its position moves.

use eframe::egui::{self, layers::ShapeIdx, LayerId, Pos2, Rect, Shape};

/// Mirrors every shape painted on `layer` since `from` around `mid_y`.
pub fn since(ctx: &egui::Context, layer: LayerId, from: ShapeIdx, mid_y: f32) {
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for i in from.0..list.next_idx().0 {
            list.mutate_shape(ShapeIdx(i), |clipped| shape(&mut clipped.shape, mid_y));
        }
    });
}

fn pos(p: &mut Pos2, mid_y: f32) {
    p.y = 2.0 * mid_y - p.y;
}

fn rect(r: Rect, mid_y: f32) -> Rect {
    Rect::from_x_y_ranges(
        r.x_range(),
        (2.0 * mid_y - r.max.y)..=(2.0 * mid_y - r.min.y),
    )
}

pub fn shape(s: &mut Shape, mid_y: f32) {
    match s {
        Shape::Noop | Shape::Callback(_) => {}
        Shape::Vec(shapes) => shapes.iter_mut().for_each(|s| shape(s, mid_y)),
        Shape::Circle(c) => pos(&mut c.center, mid_y),
        Shape::Ellipse(e) => pos(&mut e.center, mid_y),
        Shape::LineSegment { points, .. } => points.iter_mut().for_each(|p| pos(p, mid_y)),
        Shape::Path(path) => path.points.iter_mut().for_each(|p| pos(p, mid_y)),
        Shape::Rect(r) => {
            r.rect = rect(r.rect, mid_y);
            let c = &mut r.corner_radius;
            (c.nw, c.sw) = (c.sw, c.nw);
            (c.ne, c.se) = (c.se, c.ne);
        }
        Shape::Text(t) => {
            // Upright text: the box moves, the glyphs don't turn over.
            let height = t.galley.rect.height();
            t.pos.y = 2.0 * mid_y - t.pos.y - height;
        }
        Shape::Mesh(mesh) => std::sync::Arc::make_mut(mesh)
            .vertices
            .iter_mut()
            .for_each(|v| pos(&mut v.pos, mid_y)),
        Shape::QuadraticBezier(b) => b.points.iter_mut().for_each(|p| pos(p, mid_y)),
        Shape::CubicBezier(b) => b.points.iter_mut().for_each(|p| pos(p, mid_y)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, Color32, CornerRadius, Stroke};

    #[test]
    fn mirrors_around_the_middle() {
        let mut c = Shape::circle_filled(pos2(5.0, 2.0), 1.0, Color32::RED);
        shape(&mut c, 10.0);
        let Shape::Circle(c) = c else { unreachable!() };
        assert_eq!(c.center, pos2(5.0, 18.0));

        let r = Rect::from_min_max(pos2(0.0, 0.0), pos2(4.0, 6.0));
        let mut s = Shape::rect_filled(
            r,
            CornerRadius {
                nw: 1,
                ne: 2,
                sw: 3,
                se: 4,
            },
            Color32::RED,
        );
        shape(&mut s, 10.0);
        let Shape::Rect(s) = s else { unreachable!() };
        assert_eq!(s.rect, Rect::from_min_max(pos2(0.0, 14.0), pos2(4.0, 20.0)));
        assert_eq!(
            (s.corner_radius.nw, s.corner_radius.sw),
            (3, 1),
            "the rounded corners follow"
        );

        let mut line = Shape::line_segment([pos2(0.0, 0.0), pos2(1.0, 20.0)], Stroke::NONE);
        shape(&mut line, 10.0);
        let Shape::LineSegment { points, .. } = line else {
            unreachable!()
        };
        assert_eq!(points, [pos2(0.0, 20.0), pos2(1.0, 0.0)]);
    }
}
