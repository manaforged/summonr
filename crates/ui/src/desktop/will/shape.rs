use sigil::geometry::Rect;
use sigil::prelude::{Rgba8, Theme};
use sigil::scene::{SceneBuilder, StrokeCap, StrokeJoin};
use sigil::widgets::PathNode;

use super::super::style::Ink;
use super::rig::{
    BASE, BODY, EMBERS, Face, GLOW, MOTE, Mark, RING, RISE, SIG, SPARK, SPARKS, STROKE, Seg,
    ellipse,
};

/// The part of the 120 grid a canvas shows: its left, its top, and its side.
pub(super) const VIEW: (f32, f32, f32) = (-12.0, -10.0, 144.0);
/// How long a passed check's colour lap runs, and how far the edge trails the mid, in seconds.
pub(super) const SWEEP: (f32, f32) = (1.6, 0.2);

/// A lean, slant, stretch and shift about a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Hold {
    pub(super) tilt: f32,
    pub(super) skew: f32,
    pub(super) scale: (f32, f32),
    pub(super) shift: (f32, f32),
}

impl Hold {
    /// No change at all.
    pub(super) const REST: Self = Self::new(0.0, 0.0, (1.0, 1.0), (0.0, 0.0));

    /// A hold from its four parts.
    pub(super) const fn new(tilt: f32, skew: f32, scale: (f32, f32), shift: (f32, f32)) -> Self {
        Self {
            tilt,
            skew,
            scale,
            shift,
        }
    }

    /// The hold `at` of the way from this one to `other`.
    pub(super) fn mix(self, other: Self, at: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * at;
        Self::new(
            mix(self.tilt, other.tilt),
            mix(self.skew, other.skew),
            (
                mix(self.scale.0, other.scale.0),
                mix(self.scale.1, other.scale.1),
            ),
            (
                mix(self.shift.0, other.shift.0),
                mix(self.shift.1, other.shift.1),
            ),
        )
    }

    fn about(self, origin: (f32, f32)) -> Affine {
        let (sin, cos) = self.tilt.to_radians().sin_cos();
        let slant = self.skew.to_radians().tan();
        Affine::shift(-origin.0, -origin.1)
            .then(Affine([self.scale.0, 0.0, 0.0, self.scale.1, 0.0, 0.0]))
            .then(Affine([1.0, 0.0, slant, 1.0, 0.0, 0.0]))
            .then(Affine([cos, sin, -sin, cos, 0.0, 0.0]))
            .then(Affine::shift(
                origin.0 + self.shift.0,
                origin.1 + self.shift.1,
            ))
    }
}

/// A 2D affine map `[a, b, c, d, e, f]`: x' = ax + cy + e, y' = bx + dy + f.
#[derive(Clone, Copy)]
struct Affine([f32; 6]);

impl Affine {
    const fn shift(x: f32, y: f32) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, x, y])
    }

    fn then(self, outer: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [oa, ob, oc, od, oe, of] = outer.0;
        Self([
            oa * a + oc * b,
            ob * a + od * b,
            oa * c + oc * d,
            ob * c + od * d,
            oa * e + oc * f + oe,
            ob * e + od * f + of,
        ])
    }

    fn apply(self, x: f32, y: f32) -> (f32, f32) {
        let [a, b, c, d, e, f] = self.0;
        (a * x + c * y + e, b * x + d * y + f)
    }
}

/// Which status dot shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sig {
    Warn,
    Bad,
}

/// One flicker layer: its size, and the two holds it sways between.
struct Layer {
    size: f32,
    from: Hold,
    to: Hold,
}

/// The edge, mid and core: the same body at full, 0.8 and 0.6 size.
const LAYERS: [Layer; 3] = [
    Layer {
        size: 1.0,
        from: Hold::new(-1.6, 0.0, (1.0, 0.985), (0.0, 0.0)),
        to: Hold::new(1.8, 0.0, (1.0, 1.025), (0.0, 0.0)),
    },
    Layer {
        size: 0.8,
        from: Hold::new(2.0, -2.5, (1.0, 1.0), (0.0, 0.0)),
        to: Hold::new(-1.6, 1.6, (1.0, 1.04), (0.0, 0.0)),
    },
    Layer {
        size: 0.6,
        from: Hold::new(0.0, 0.0, (0.975, 0.965), (0.0, 0.0)),
        to: Hold::new(0.0, 0.0, (1.025, 1.045), (0.0, 0.0)),
    },
];

/// Everything that sets one drawn frame of Will.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) pose: Hold,
    pub(super) react: Hold,
    pub(super) fade: f32,
    pub(super) sway: [f32; 3],
    pub(super) bob: f32,
    pub(super) glow: (f32, f32),
    pub(super) ring: f32,
    pub(super) gaze: (f32, f32),
    pub(super) face: Face,
    pub(super) was: Face,
    pub(super) mix: f32,
    pub(super) blink: f32,
    pub(super) embers: (f32, f32),
    pub(super) sparks: Option<f32>,
    pub(super) mote: Option<f32>,
    pub(super) sig: Option<(Sig, f32, f32)>,
    pub(super) sweep: Option<f32>,
}

/// Paints `frame` into `area` in `ink`.
pub(super) fn draw(scene: &mut SceneBuilder, area: Rect, frame: &Frame, ink: &Ink, theme: &Theme) {
    let view = Affine::shift(-VIEW.0, -VIEW.1);
    let mut paint = |node: PathNode| node.draw(scene, area, theme);
    if frame.ring > 0.01 {
        let (cx, cy, rx, ry) = RING;
        let at = Hold::new(0.0, 0.0, (frame.ring, 1.0), (0.0, 0.0))
            .about((cx, cy))
            .then(view);
        paint(trace(&ellipse(cx, cy, rx, ry), at).stroke(alpha(ink.mid, frame.ring * 0.6), 1.3));
    }
    let (glow, grow) = frame.glow;
    if glow > 0.01 {
        let (cx, cy, r) = GLOW;
        let at = view;
        let center = at.apply(cx, cy);
        let stops = vec![
            (0.0, alpha(ink.glow, 0.5 * glow)),
            (0.55, alpha(ink.glow, 0.13 * glow)),
            (1.0, alpha(ink.glow, 0.0)),
        ];
        paint(trace(&ellipse(cx, cy, r * grow, r * grow), at).fill_radial(center, r * grow, stops));
    }
    let body = frame
        .pose
        .about(BASE)
        .then(Affine::shift(0.0, frame.bob))
        .then(frame.react.about(BASE))
        .then(view);
    let (edge, mid) = frame.sweep.map_or((ink.edge, ink.mid), |time| {
        (
            turn(ink.edge, lap(time - SWEEP.1)),
            turn(ink.mid, lap(time)),
        )
    });
    let mut core = Hold::REST;
    for (layer, (color, at)) in LAYERS
        .iter()
        .zip([edge, mid, ink.core].into_iter().zip(frame.sway))
    {
        let held = layer.from.mix(layer.to, at);
        let size = Hold::new(0.0, 0.0, (layer.size, layer.size), (0.0, 0.0));
        let place = size.about(BASE).then(held.about(BASE)).then(body);
        paint(trace(&BODY, place).fill(alpha(color, frame.fade)));
        core = held;
    }
    let eyes = |face: Face| {
        Hold::new(0.0, 0.0, (1.0, frame.blink), (0.0, 0.0))
            .about((BASE.0, face.middle()))
            .then(Affine::shift(frame.gaze.0, frame.gaze.1))
            .then(core.about(BASE))
            .then(body)
    };
    if frame.mix < 1.0 && frame.was != frame.face {
        face(
            &mut paint,
            frame.was,
            eyes(frame.was),
            alpha(ink.eye, frame.fade * (1.0 - frame.mix)),
        );
    }
    face(
        &mut paint,
        frame.face,
        eyes(frame.face),
        alpha(ink.eye, frame.fade * frame.mix),
    );
    let (lit, time) = frame.embers;
    if lit > 0.01 {
        for (x, y, r, pass, wait, drift) in EMBERS {
            if time < wait {
                continue;
            }
            let at = ((time - wait) % pass) / pass;
            let shown = if at < 0.12 {
                at / 0.12
            } else {
                1.0 - (at - 0.12) / 0.88
            };
            let r = r * (1.0 - 0.7 * at);
            let dot = ellipse(x + drift * at, y - RISE * at, r, r);
            paint(trace(&dot, view).fill(alpha(mid, lit * shown)));
        }
    }
    if let Some(out) = frame.sparks {
        let (cx, cy, side) = SPARK;
        let half = side * std::f32::consts::FRAC_1_SQRT_2 * (0.2 + 0.8 * out);
        for (dx, dy) in SPARKS {
            let (x, y) = (cx + dx * out, cy + dy * out);
            let diamond = [
                Seg::Move(x, y - half),
                Seg::Line(x + half, y),
                Seg::Line(x, y + half),
                Seg::Line(x - half, y),
                Seg::Close,
            ];
            paint(trace(&diamond, view).fill(alpha(mid, 1.0 - out)));
        }
    }
    if let Some(at) = frame.mote {
        let (x, y, r, (dx, dy)) = MOTE;
        let shown = if at < 0.3 {
            0.8 * at / 0.3
        } else {
            0.8 * (1.0 - (at - 0.3) / 0.7)
        };
        paint(trace(&ellipse(x + dx * at, y + dy * at, r, r), view).fill(alpha(mid, shown)));
    }
    if let Some((sig, shown, grow)) = frame.sig {
        let (x, y, r) = SIG;
        let color = match sig {
            Sig::Warn => ink.warn,
            Sig::Bad => ink.bad,
        };
        paint(trace(&ellipse(x, y, r * grow, r * grow), view).fill(alpha(color, shown)));
    }
}

fn face(paint: &mut impl FnMut(PathNode), face: Face, at: Affine, color: Rgba8) {
    if color.a == 0 {
        return;
    }
    for mark in face.marks() {
        paint(match mark {
            Mark::Fill(segs) => trace(segs, at).fill(color),
            Mark::Line(segs) => trace(segs, at)
                .stroke(color, STROKE)
                .cap(StrokeCap::Round)
                .join(StrokeJoin::Round),
        });
    }
}

/// How far a colour lap has turned the hue at `time` seconds: up 35°, on to 70°, and back.
fn lap(time: f32) -> f32 {
    let at = (time / SWEEP.0).clamp(0.0, 1.0);
    let ease = |x: f32| (1.0 - (x * std::f32::consts::PI).cos()) / 2.0;
    match at {
        at if at < 0.4 => 35.0 * ease(at / 0.4),
        at if at < 0.65 => 35.0 + 35.0 * ease((at - 0.4) / 0.25),
        at => 70.0 * (1.0 - ease((at - 0.65) / 0.35)),
    }
}

fn alpha(color: Rgba8, by: f32) -> Rgba8 {
    Rgba8 {
        a: (f32::from(color.a) * by.clamp(0.0, 1.0)).round() as u8,
        ..color
    }
}

/// `color` with its hue turned by `degrees`, keeping its saturation and lightness.
fn turn(color: Rgba8, degrees: f32) -> Rgba8 {
    if degrees.abs() < 0.01 {
        return color;
    }
    let [r, g, b] = [color.r, color.g, color.b].map(|each| f32::from(each) / 255.0);
    let (most, least) = (r.max(g).max(b), r.min(g).min(b));
    let light = (most + least) / 2.0;
    let spread = most - least;
    if spread < 1e-4 {
        return color;
    }
    let sat = spread / (1.0 - (2.0 * light - 1.0).abs());
    let hue = if (most - r).abs() < 1e-6 {
        ((g - b) / spread).rem_euclid(6.0)
    } else if (most - g).abs() < 1e-6 {
        (b - r) / spread + 2.0
    } else {
        (r - g) / spread + 4.0
    } * 60.0;
    let hue = (hue + degrees).rem_euclid(360.0) / 60.0;
    let chroma = (1.0 - (2.0 * light - 1.0).abs()) * sat;
    let x = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hue as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        5..=u8::MAX => (chroma, 0.0, x),
    };
    let lift = light - chroma / 2.0;
    let byte = |each: f32| ((each + lift).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba8 {
        r: byte(r),
        g: byte(g),
        b: byte(b),
        a: color.a,
    }
}

fn trace(segs: &[Seg], at: Affine) -> PathNode {
    let side = VIEW.2 as u32;
    segs.iter()
        .fold(sigil::widgets::path(side, side), |node, seg| match *seg {
            Seg::Move(x, y) => {
                let (x, y) = at.apply(x, y);
                node.move_to(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = at.apply(x, y);
                node.line_to(x, y)
            }
            Seg::Quad([cx, cy, x, y]) => {
                let ((cx, cy), (x, y)) = (at.apply(cx, cy), at.apply(x, y));
                node.quad_to(cx, cy, x, y)
            }
            Seg::Curve([ax, ay, bx, by, x, y]) => {
                let ((ax, ay), (bx, by), (x, y)) =
                    (at.apply(ax, ay), at.apply(bx, by), at.apply(x, y));
                node.cubic_to(ax, ay, bx, by, x, y)
            }
            Seg::Close => node.close(),
        })
}
