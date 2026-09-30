//! Lulo's mascot: a minimalist octopus drawn with vector shapes, with one
//! animated scene per session state. Scenes are designed on a 100×100
//! canvas (the same coordinates as the design mockup) and scaled into
//! whatever rect they are given.

use std::f64::consts::TAU;

use eframe::egui::{pos2, vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

use crate::style;

const EYE: Color32 = Color32::from_rgb(18, 18, 23);
const INK: Color32 = style::TEXT;
const SCREEN: Color32 = Color32::from_rgb(28, 28, 36);
const FRAME: Color32 = Color32::from_rgb(58, 58, 72);
const DESK: Color32 = Color32::from_rgb(42, 42, 53);
const KEYS: Color32 = Color32::from_rgb(68, 68, 90);

/// A cubic segment: two control points and the end point.
type Seg = [f32; 6];

const ARMS: [([f32; 2], [Seg; 2]); 4] = [
    (
        [36., 56.],
        [
            [30., 68., 16., 74., 11., 65.],
            [8., 59., 14., 56., 17., 61.],
        ],
    ),
    (
        [45., 60.],
        [
            [44., 72., 38., 82., 30., 82.],
            [25., 82., 25., 76., 29., 76.],
        ],
    ),
    (
        [55., 60.],
        [
            [56., 72., 62., 82., 70., 82.],
            [75., 82., 75., 76., 71., 76.],
        ],
    ),
    (
        [64., 56.],
        [
            [70., 68., 84., 74., 89., 65.],
            [92., 59., 86., 56., 83., 61.],
        ],
    ),
];

/// Rounded-square head. Convex, so it fills as a single polygon.
const HEAD: ([f32; 2], [Seg; 6]) = (
    [50., 10.],
    [
        [70., 10., 76., 18., 76., 34.],
        [76., 34., 76., 46., 76., 46.],
        [76., 58., 68., 62., 50., 62.],
        [32., 62., 24., 58., 24., 46.],
        [24., 46., 24., 34., 24., 34.],
        [24., 18., 30., 10., 50., 10.],
    ],
);

/// Draws the octopus scene for `state` into `rect` at time `t` (seconds).
pub fn paint(painter: &Painter, rect: Rect, state: &str, t: f64) {
    let size = rect.width().min(rect.height());
    let origin = rect.center() - vec2(size, size) / 2.0;
    let pen = Pen {
        painter,
        m: Affine::translate(origin.x, origin.y).then(Affine::scale(size / 100.0, size / 100.0)),
    };
    let c = style::look(state).color;
    match state {
        "thinking" => thinking(pen, t, c),
        "editing" => editing(pen, t, c),
        "bash" => bash(pen, t, c),
        "reading" => reading(pen, t, c),
        "subagent" => subagent(pen, t, c),
        "waiting" => waiting(pen, t, c),
        "done" => done(pen, t, c),
        "error" => error(pen, t, c),
        "inactive" => sleeping(pen, t, c),
        _ => idle(pen, t, c),
    }
}

/// The state the collapsed strip shows: the one that most needs the user.
pub fn most_urgent<'a>(states: impl IntoIterator<Item = &'a str>) -> &'a str {
    let rank = |s: &str| match s {
        "waiting" => 0,
        "error" => 1,
        "editing" | "bash" | "reading" | "subagent" | "thinking" | "tool" => 2,
        "done" => 3,
        "ready" => 4,
        _ => 5,
    };
    states
        .into_iter()
        .min_by_key(|s| rank(s))
        .unwrap_or("inactive")
}

// ---- Scenes ----

/// Sways side to side looking up while idea dots pop up one after another.
fn thinking(pen: Pen, t: f64, c: Color32) {
    let rock = pen.with(Affine::about(50., 86., Affine::rotate(3. * swing(t, 3.))));
    body(rock, c);
    eyes(rock, t, 3., -3.);
    for (i, (x, y, r)) in [(83., 20., 2.4), (90., 10., 3.2), (98., -2., 4.2)]
        .into_iter()
        .enumerate()
    {
        let p = phase(t, 2.4, 0.3 * i as f64);
        let alpha = keys(p, &[(0., 0.), (0.12, 0.), (0.28, 1.), (0.78, 1.), (1., 0.)]);
        let scale = keys(p, &[(0., 0.3), (0.12, 0.3), (0.28, 1.), (1., 1.)]);
        pen.circle(x, y, r * scale, c.gamma_multiply(alpha));
    }
}

/// Seen from behind, sitting at a PC and typing while code appears.
fn editing(pen: Pen, t: f64, c: Color32) {
    pen.rrect(18., 4., 64., 40., 4., SCREEN);
    pen.outline(&rounded(18., 4., 64., 40., 4.), 2.5, FRAME);
    code_lines(
        pen,
        t,
        &[
            (24., 10., 20., c),
            (28., 17., 34., dim(0.55)),
            (28., 24., 26., dim(0.55)),
        ],
    );
    cursor(pen, t, 56., 23., c);
    pen.rrect(46., 44., 8., 8., 0., FRAME);
    pen.rrect(4., 54., 92., 3., 1.5, DESK);
    pen.rrect(22., 49., 56., 5., 1.5, KEYS);

    // Two arms reach the keyboard and tap in turn.
    let tap = |shift: f64| -3.5 * pulse(t + shift, 0.34);
    pen.with(Affine::translate(0., tap(0.))).stroke(
        &curve([38., 62.], &[[30., 62., 25., 58., 26., 51.]]),
        6.,
        c,
    );
    pen.with(Affine::translate(0., tap(0.17))).stroke(
        &curve([62., 62.], &[[70., 62., 75., 58., 74., 51.]]),
        6.,
        c,
    );
    for (x, delay) in [(26., 0.), (71., 0.34)] {
        let p = phase(t, 0.68, delay);
        let alpha = keys(p, &[(0., 0.), (0.3, 1.), (1., 0.)]);
        pen.rrect(x, 42. - 9. * p, 3., 3., 1., INK.gamma_multiply(alpha));
    }

    let me = pen
        .with(Affine::translate(14., 33.).then(Affine::scale(0.72, 0.72)))
        .with(Affine::translate(0., pulse(t, 0.68)));
    body(me, c);
    // Screen light on the back of the head.
    me.stroke(
        &curve([34., 16.], &[[42., 12., 58., 12., 66., 16.]]),
        3.,
        INK.gamma_multiply(0.35),
    );
}

/// Watches a terminal where command lines type themselves.
fn bash(pen: Pen, t: f64, c: Color32) {
    let me = pen.with(Affine::translate(-8., 20.).then(Affine::scale(0.78, 0.78)));
    let rock = me.with(Affine::about(50., 86., Affine::rotate(3. * swing(t, 3.))));
    body(rock, c);
    eyes(rock, t, 3., -3.);

    pen.rrect(48., 2., 50., 38., 5., SCREEN);
    pen.outline(&rounded(48., 2., 50., 38., 5.), 2., c);
    pen.circle(54., 8., 1.5, c.gamma_multiply(0.6));
    pen.circle(59., 8., 1.5, c.gamma_multiply(0.6));
    pen.stroke(&[pos2(54., 16.), pos2(58., 19.), pos2(54., 22.)], 2., c);
    code_lines(
        pen,
        t,
        &[
            (61., 17.5, 24., dim(0.85)),
            (54., 25., 32., dim(0.45)),
            (54., 32., 18., dim(0.45)),
        ],
    );
    cursor(pen, t, 75., 31., c);
}

/// Sweeps a magnifying glass side to side; the eyes follow it.
fn reading(pen: Pen, t: f64, c: Color32) {
    let sweep = swing(t, 5.2);
    body(pen, c);
    eyes(pen.with(Affine::translate(3. * sweep, 0.)), t, 0., 0.);
    let lens = pen.with(Affine::translate(13. * sweep, 0.));
    lens.circle(50., 43., 11., INK.gamma_multiply(0.14));
    lens.ring(50., 43., 11., 3., INK);
    lens.stroke(&[pos2(58., 51.), pos2(67., 60.)], 4.5, INK);
}

/// Two little helper octopuses bounce beside it, linked by dashed lines.
fn subagent(pen: Pen, t: f64, c: Color32) {
    let offset = 8. * phase(t, 1.2, 0.);
    for (start, ctrl, end) in [
        ([30., 58.], [22., 70.], [16., 72.]),
        ([70., 58.], [78., 70.], [84., 72.]),
    ] {
        dashed(
            pen,
            &curve(start, &[quad(start, ctrl, end)]),
            2.,
            c.gamma_multiply(0.7),
            offset,
        );
    }
    let main = pen.with(Affine::translate(15., 6.).then(Affine::scale(0.7, 0.7)));
    body(main, c);
    eyes(main, t, 0., 0.);
    for (x, delay) in [(-4., 0.), (68., -0.6)] {
        let p = phase(t, 1.2, delay);
        let lift = keys(p, &[(0., 0.), (0.4, -5.), (0.7, 0.), (1., 0.)]);
        let sx = keys(p, &[(0., 1.), (0.4, 0.95), (0.7, 1.06), (1., 1.)]);
        let sy = keys(p, &[(0., 1.), (0.4, 1.05), (0.7, 0.94), (1., 1.)]);
        let baby = pen
            .with(Affine::translate(x, 62.).then(Affine::scale(0.36, 0.36)))
            .with(Affine::about(
                50.,
                86.,
                Affine::translate(0., lift).then(Affine::scale(sx, sy)),
            ));
        body(baby, c);
        eyes(baby, t, 0., 0.);
    }
}

/// Impatient: frowning, tapping the floor with one arm, a clock spinning.
fn waiting(pen: Pen, t: f64, c: Color32) {
    let beat = pulse(t, 0.44);
    let me = pen.with(Affine::translate(0., -1.8 * beat));
    for i in 0..3 {
        arm(me, i, c);
    }
    head(me, c);
    let glance = keys(
        phase(t, 3., 0.),
        &[(0., 0.), (0.4, 0.), (0.5, -4.), (0.8, -4.), (1., 0.)],
    );
    let face = me.with(Affine::translate(glance, 0.));
    for x in [40., 60.] {
        face.rrect(x - 2.7, 38., 5.4, 9.4, 2.7, EYE);
    }
    // Head-colored lids cut the eyes flat, and the brows slant in.
    face.rrect(33., 35., 34., 7., 0., c);
    face.stroke(&[pos2(35., 36.), pos2(45., 38.5)], 2.4, EYE);
    face.stroke(&[pos2(65., 36.), pos2(55., 38.5)], 2.4, EYE);
    arm(
        pen.with(Affine::about(64., 56., Affine::rotate(-16. * beat))),
        3,
        c,
    );

    let clock = pen.with(Affine::translate(12., 12.));
    clock.circle(0., 0., 10., SCREEN);
    clock.ring(0., 0., 10., 2.5, INK);
    clock.with(Affine::rotate(360. * phase(t, 1.3, 0.))).stroke(
        &[pos2(0., -6.5), pos2(0., 0.)],
        2.5,
        c,
    );
    clock.circle(0., 0., 1.6, INK);
}

/// Jumps for joy with raised arms while sparkles pop around it.
fn done(pen: Pen, t: f64, c: Color32) {
    let p = phase(t, 1.2, 0.);
    let shadow = keys(p, &[(0., 1.), (0.3, 0.65), (0.55, 1.), (1., 1.)]);
    pen.fill(
        &ellipse(50., 90., 22. * shadow, 3.),
        INK.gamma_multiply(0.12),
    );

    let lift = keys(p, &[(0., 0.), (0.3, -10.), (0.55, 0.), (1., 0.)]);
    let sx = keys(
        p,
        &[(0., 1.06), (0.3, 0.96), (0.55, 1.07), (0.7, 1.), (1., 1.)],
    );
    let sy = keys(
        p,
        &[(0., 0.94), (0.3, 1.05), (0.55, 0.93), (0.7, 1.), (1., 1.)],
    );
    let me = pen.with(Affine::about(
        50.,
        86.,
        Affine::translate(0., lift).then(Affine::scale(sx, sy)),
    ));
    arm(me, 1, c);
    arm(me, 2, c);
    me.stroke(
        &curve(
            [30., 56.],
            &[
                [16., 52., 8., 42., 10., 32.],
                [11., 25., 18., 25., 17., 31.],
            ],
        ),
        8.,
        c,
    );
    me.stroke(
        &curve(
            [70., 56.],
            &[
                [84., 52., 92., 42., 90., 32.],
                [89., 25., 82., 25., 83., 31.],
            ],
        ),
        8.,
        c,
    );
    head(me, c);
    for x in [36., 56.] {
        me.stroke(
            &curve([x, 44.], &[quad([x, 44.], [x + 4., 38.], [x + 8., 44.])]),
            3.2,
            EYE,
        );
    }

    for (i, (x, y, color)) in [(8., 40., c), (92., 36., INK), (14., 8., INK), (88., 6., c)]
        .into_iter()
        .enumerate()
    {
        let p = phase(t, 1.2, 0.3 * i as f64);
        let alpha = keys(p, &[(0., 0.), (0.3, 1.), (0.6, 0.), (1., 0.)]);
        let scale = keys(p, &[(0., 0.), (0.3, 1.), (0.6, 0.6), (1., 0.6)]);
        let turn = keys(p, &[(0., 0.), (0.3, 45.), (0.6, 90.), (1., 90.)]);
        let star = pen.with(
            Affine::translate(x, y)
                .then(Affine::rotate(turn))
                .then(Affine::scale(scale, scale)),
        );
        let color = color.gamma_multiply(alpha);
        star.fill(
            &[pos2(0., -5.), pos2(1.4, 0.), pos2(0., 5.), pos2(-1.4, 0.)],
            color,
        );
        star.fill(
            &[pos2(-5., 0.), pos2(0., -1.4), pos2(5., 0.), pos2(0., 1.4)],
            color,
        );
    }
}

/// Shakes with X eyes while smoke puffs rise from its head.
fn error(pen: Pen, t: f64, c: Color32) {
    let p = phase(t, 1.8, 0.);
    let mut shake = vec![(0., 0.)];
    shake.extend((1..=8).map(|i| (0.05 * i as f32, if i % 2 == 1 { -2.5 } else { 2.5 })));
    shake.extend([(0.45, 0.), (1., 0.)]);
    let me = pen.with(Affine::translate(keys(p, &shake), 0.));
    body(me, c);
    for x in [37., 57.] {
        me.stroke(&[pos2(x, 39.), pos2(x + 6., 46.)], 3., EYE);
        me.stroke(&[pos2(x + 6., 39.), pos2(x, 46.)], 3., EYE);
    }
    for (i, (x, y, r)) in [(44., 4., 4.), (54., 2., 5.), (49., 0., 3.5)]
        .into_iter()
        .enumerate()
    {
        let p = phase(t, 1.8, 0.5 * i as f64);
        let alpha = keys(p, &[(0., 0.), (0.25, 0.5), (1., 0.)]);
        let scale = keys(p, &[(0., 0.4), (1., 1.2)]);
        pen.circle(x, y - 14. * p, r * scale, INK.gamma_multiply(alpha));
    }
}

/// Sleeps, breathing slowly, with Zs floating up.
fn sleeping(pen: Pen, t: f64, c: Color32) {
    let breath = pulse(t, 4.);
    let me = pen.with(Affine::about(
        50.,
        86.,
        Affine::scale(1. + 0.03 * breath, 1. - 0.04 * breath),
    ));
    body(me, c);
    me.stroke(&[pos2(36., 44.), pos2(44., 44.)], 3., EYE);
    me.stroke(&[pos2(56., 44.), pos2(64., 44.)], 3., EYE);
    for (i, size) in [1., 0.8, 0.65].into_iter().enumerate() {
        let p = phase(t, 3.6, 1.2 * i as f64);
        let alpha = keys(p, &[(0., 0.), (0.2, 1.), (1., 0.)]);
        let z = pen.with(
            Affine::translate(76., 14.)
                .then(Affine::scale(size, size))
                .then(Affine::translate(9. * p, -18. * p)),
        );
        let zig = [pos2(0., 0.), pos2(7., 0.), pos2(0., 7.), pos2(7., 7.)];
        z.stroke(&zig, 2.4, c.gamma_multiply(alpha));
    }
}

/// Ready or anything unknown: just there, breathing and blinking.
fn idle(pen: Pen, t: f64, c: Color32) {
    let breath = pulse(t, 4.);
    let me = pen.with(Affine::about(
        50.,
        86.,
        Affine::scale(1. + 0.02 * breath, 1. - 0.02 * breath),
    ));
    body(me, c);
    eyes(me, t, 0., 0.);
}

// ---- Octopus parts ----

fn arm(pen: Pen, i: usize, c: Color32) {
    let (start, segs) = &ARMS[i];
    pen.stroke(&curve(*start, segs), 8., c);
}

fn head(pen: Pen, c: Color32) {
    let mut outline = curve(HEAD.0, &HEAD.1);
    outline.pop();
    pen.fill(&outline, c);
}

fn body(pen: Pen, c: Color32) {
    for i in 0..ARMS.len() {
        arm(pen, i, c);
    }
    head(pen, c);
}

/// Pill-shaped eyes, shifted by (dx, dy), blinking every few seconds.
fn eyes(pen: Pen, t: f64, dx: f32, dy: f32) {
    let open = keys(
        phase(t, 4., 0.),
        &[(0., 1.), (0.9, 1.), (0.94, 0.1), (1., 1.)],
    );
    let pen = pen.with(Affine::about(50., 42.7 + dy, Affine::scale(1., open)));
    for x in [40., 60.] {
        pen.rrect(x + dx - 2.7, 38. + dy, 5.4, 9.4, 2.7, EYE);
    }
}

/// Lines of text that type themselves in, one after another.
fn code_lines(pen: Pen, t: f64, lines: &[(f32, f32, f32, Color32)]) {
    for (i, &(x, y, w, color)) in lines.iter().enumerate() {
        let p = phase(t, 3., 0.6 * i as f64);
        let typed = keys(p, &[(0., 0.), (0.18, 1.), (1., 1.)]);
        let alpha = keys(p, &[(0., 1.), (0.88, 1.), (1., 0.)]);
        if typed * w > 0.5 {
            pen.rrect(x, y, w * typed, 3., 1.5, color.gamma_multiply(alpha));
        }
    }
}

fn cursor(pen: Pen, t: f64, x: f32, y: f32, c: Color32) {
    if phase(t, 1., 0.) < 0.5 {
        pen.rrect(x, y, 3., 5., 0., c);
    }
}

fn dim(alpha: f32) -> Color32 {
    INK.gamma_multiply(alpha)
}

// ---- Timing ----

/// Position in a looping animation, 0..1.
fn phase(t: f64, period: f64, delay: f64) -> f32 {
    ((t - delay).rem_euclid(period) / period) as f32
}

/// Smooth back-and-forth, -1..1, starting at -1.
fn swing(t: f64, period: f64) -> f32 {
    -(TAU * t / period).cos() as f32
}

/// Smooth 0 → 1 → 0 once per period.
fn pulse(t: f64, period: f64) -> f32 {
    (1. - (TAU * t / period).cos() as f32) / 2.
}

/// Keyframes like CSS: eased interpolation between (position, value) pairs.
fn keys(p: f32, frames: &[(f32, f32)]) -> f32 {
    for w in frames.windows(2) {
        let ((p0, v0), (p1, v1)) = (w[0], w[1]);
        if p <= p1 {
            if p1 <= p0 {
                return v1;
            }
            let x = ((p - p0) / (p1 - p0)).clamp(0., 1.);
            return v0 + (v1 - v0) * x * x * (3. - 2. * x);
        }
    }
    frames.last().map_or(0., |f| f.1)
}

// ---- Geometry ----

/// 2D affine transform: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Affine([f32; 6]);

impl Affine {
    fn translate(x: f32, y: f32) -> Self {
        Affine([1., 0., 0., 1., x, y])
    }

    fn scale(sx: f32, sy: f32) -> Self {
        Affine([sx, 0., 0., sy, 0., 0.])
    }

    /// Clockwise on screen, like CSS `rotate()`.
    fn rotate(deg: f32) -> Self {
        let (s, c) = deg.to_radians().sin_cos();
        Affine([c, s, -s, c, 0., 0.])
    }

    /// `m` applied around the point (x, y) instead of the origin.
    fn about(x: f32, y: f32, m: Affine) -> Self {
        Affine::translate(x, y)
            .then(m)
            .then(Affine::translate(-x, -y))
    }

    /// Points go through `inner` first, then `self`.
    fn then(self, inner: Affine) -> Affine {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = inner.0;
        Affine([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ])
    }

    fn apply(&self, p: Pos2) -> Pos2 {
        let [a, b, c, d, e, f] = self.0;
        pos2(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// How much lengths grow on average, for stroke widths and radii.
    fn unit(&self) -> f32 {
        let [a, b, c, d, ..] = self.0;
        (a * d - b * c).abs().sqrt()
    }
}

/// Paints through a transform, in scene units.
#[derive(Clone, Copy)]
struct Pen<'a> {
    painter: &'a Painter,
    m: Affine,
}

impl<'a> Pen<'a> {
    fn with(self, m: Affine) -> Pen<'a> {
        Pen {
            painter: self.painter,
            m: self.m.then(m),
        }
    }

    fn map(&self, pts: &[Pos2]) -> Vec<Pos2> {
        pts.iter().map(|p| self.m.apply(*p)).collect()
    }

    /// Open line with round ends.
    fn stroke(&self, pts: &[Pos2], width: f32, color: Color32) {
        if pts.len() < 2 || color.a() == 0 {
            return;
        }
        let w = width * self.m.unit();
        let pts = self.map(pts);
        // Caps on translucent lines would show as darker dots where they overlap.
        if color.a() == 255 {
            self.painter.circle_filled(pts[0], w / 2., color);
            self.painter
                .circle_filled(pts[pts.len() - 1], w / 2., color);
        }
        self.painter.add(Shape::line(pts, Stroke::new(w, color)));
    }

    fn outline(&self, pts: &[Pos2], width: f32, color: Color32) {
        let stroke = Stroke::new(width * self.m.unit(), color);
        self.painter.add(Shape::closed_line(self.map(pts), stroke));
    }

    /// Fills a convex polygon.
    fn fill(&self, pts: &[Pos2], color: Color32) {
        if color.a() == 0 {
            return;
        }
        // Repeated or nearly repeated vertices (where the arcs of a pill meet,
        // or a squashed blink) break the anti-aliased edge into spikes.
        let mut outline: Vec<Pos2> = Vec::with_capacity(pts.len());
        for p in self.map(pts) {
            if outline.last().is_none_or(|q| q.distance(p) > 0.05) {
                outline.push(p);
            }
        }
        while outline.len() > 1 && outline[0].distance(outline[outline.len() - 1]) <= 0.05 {
            outline.pop();
        }
        if outline.len() >= 3 {
            self.painter
                .add(Shape::convex_polygon(outline, color, Stroke::NONE));
        }
    }

    fn rrect(&self, x: f32, y: f32, w: f32, h: f32, r: f32, color: Color32) {
        self.fill(&rounded(x, y, w, h, r), color);
    }

    fn circle(&self, x: f32, y: f32, r: f32, color: Color32) {
        if color.a() > 0 {
            let center = self.m.apply(pos2(x, y));
            self.painter.circle_filled(center, r * self.m.unit(), color);
        }
    }

    fn ring(&self, x: f32, y: f32, r: f32, width: f32, color: Color32) {
        let unit = self.m.unit();
        let center = self.m.apply(pos2(x, y));
        self.painter
            .circle_stroke(center, r * unit, Stroke::new(width * unit, color));
    }
}

/// Samples a chain of cubic segments into a polyline.
fn curve(start: [f32; 2], segs: &[Seg]) -> Vec<Pos2> {
    const STEPS: usize = 16;
    let mut pts = vec![pos2(start[0], start[1])];
    let mut p0 = pts[0];
    for s in segs {
        let (p1, p2, p3) = (pos2(s[0], s[1]), pos2(s[2], s[3]), pos2(s[4], s[5]));
        for i in 1..=STEPS {
            let t = i as f32 / STEPS as f32;
            let u = 1. - t;
            let p = p0.to_vec2() * (u * u * u)
                + p1.to_vec2() * (3. * u * u * t)
                + p2.to_vec2() * (3. * u * t * t)
                + p3.to_vec2() * (t * t * t);
            pts.push(p.to_pos2());
        }
        p0 = p3;
    }
    pts
}

/// A quadratic segment written as the equivalent cubic.
fn quad(p0: [f32; 2], ctrl: [f32; 2], end: [f32; 2]) -> Seg {
    let k = 2. / 3.;
    [
        p0[0] + k * (ctrl[0] - p0[0]),
        p0[1] + k * (ctrl[1] - p0[1]),
        end[0] + k * (ctrl[0] - end[0]),
        end[1] + k * (ctrl[1] - end[1]),
        end[0],
        end[1],
    ]
}

fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Pos2> {
    let r = r.min(w / 2.).min(h / 2.).max(0.);
    if r == 0. {
        return vec![
            pos2(x, y),
            pos2(x + w, y),
            pos2(x + w, y + h),
            pos2(x, y + h),
        ];
    }
    let corners = [
        (x + w - r, y + r, -90.),
        (x + w - r, y + h - r, 0.),
        (x + r, y + h - r, 90.),
        (x + r, y + r, 180.),
    ];
    let mut pts = Vec::with_capacity(24);
    for (cx, cy, from) in corners {
        for i in 0..=5 {
            let a = (from + 18. * i as f32).to_radians();
            pts.push(pos2(cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    pts
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Pos2> {
    (0..24)
        .map(|i| {
            let a = i as f32 / 24. * std::f32::consts::TAU;
            pos2(cx + rx * a.cos(), cy + ry * a.sin())
        })
        .collect()
}

/// Dashes (3 on, 5 off) along a polyline, shifted by `offset` to crawl.
fn dashed(pen: Pen, pts: &[Pos2], width: f32, color: Color32, offset: f32) {
    let mut run: Vec<Pos2> = Vec::new();
    let mut dist = 0.;
    for w in pts.windows(2) {
        if (dist - offset).rem_euclid(8.) < 3. {
            if run.is_empty() {
                run.push(w[0]);
            }
            run.push(w[1]);
        } else if !run.is_empty() {
            pen.stroke(&run, width, color);
            run.clear();
        }
        dist += w[0].distance(w[1]);
    }
    if !run.is_empty() {
        pen.stroke(&run, width, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, LayerId};

    #[test]
    fn transforms_compose_inner_first() {
        let m = Affine::translate(10., 0.).then(Affine::scale(2., 2.));
        assert_eq!(m.apply(pos2(1., 1.)), pos2(12., 2.));
        let r = Affine::about(50., 50., Affine::rotate(90.));
        let p = r.apply(pos2(60., 50.));
        assert!(
            (p.x - 50.).abs() < 1e-4 && (p.y - 60.).abs() < 1e-4,
            "{p:?}"
        );
        assert!((Affine::scale(0.5, 0.5).unit() - 0.5).abs() < 1e-6);
    }

    #[test]
    fn keyframes_hit_their_values() {
        let k = [(0., 0.), (0.5, 10.), (1., 0.)];
        assert_eq!(keys(0., &k), 0.);
        assert_eq!(keys(0.5, &k), 10.);
        assert_eq!(keys(0.25, &k), 5.);
        assert_eq!(keys(1., &k), 0.);
        assert!((phase(5.5, 2., 0.) - 0.75).abs() < 1e-6);
        assert!((phase(0.1, 1., 0.3) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn picks_the_state_that_needs_the_user() {
        assert_eq!(most_urgent(["done", "waiting", "editing"]), "waiting");
        assert_eq!(most_urgent(["inactive", "bash"]), "bash");
        assert_eq!(most_urgent(["inactive", "done"]), "done");
        assert_eq!(most_urgent([]), "inactive");
    }

    #[test]
    fn every_scene_paints_inside_its_box() {
        let ctx = Context::default();
        let painter = Painter::new(ctx, LayerId::background(), Rect::EVERYTHING);
        let rect = Rect::from_min_size(pos2(0., 0.), vec2(100., 100.));
        let states = [
            "thinking", "editing", "bash", "reading", "subagent", "waiting", "done", "error",
            "inactive", "ready", "tool",
        ];
        for state in states {
            for t in [0., 0.37, 1.9, 7.3] {
                paint(&painter, rect, state, t);
            }
        }
        let bounds = painter.ctx().graphics(|g| {
            g.get(LayerId::background()).map(|list| {
                list.all_entries()
                    .map(|s| s.shape.visual_bounding_rect())
                    .fold(Rect::NOTHING, Rect::union)
            })
        });
        let bounds = bounds.expect("scenes drew nothing");
        assert!(bounds.expand(0.1).intersects(rect));
        assert!(
            Rect::from_min_max(pos2(-10., -20.), pos2(110., 110.)).contains_rect(bounds),
            "{bounds:?}"
        );
    }
}
