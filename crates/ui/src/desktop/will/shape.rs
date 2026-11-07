use sigil::geometry::Rect;
use sigil::prelude::{Rgba8, Theme};
use sigil::scene::SceneBuilder;
use sigil::widgets::PathNode;

/// The point Will scales and leans about, on the 16 grid.
const BASE: (f32, f32) = (8.0, 15.0);

#[derive(Clone, Copy)]
enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Curve([f32; 6]),
    Close,
}

const FLAME: [Seg; 9] = [
    Seg::Move(7.3, 1.8),
    Seg::Curve([7.8, 3.6, 9.6, 4.2, 11.0, 5.2]),
    Seg::Curve([12.2, 6.1, 13.0, 7.4, 13.0, 9.0]),
    Seg::Curve([13.0, 11.76, 10.76, 14.0, 8.0, 14.0]),
    Seg::Curve([5.24, 14.0, 3.0, 11.76, 3.0, 9.0]),
    Seg::Curve([3.0, 7.6, 3.5, 6.6, 4.2, 5.8]),
    Seg::Curve([4.6, 6.6, 5.2, 6.8, 5.8, 6.7]),
    Seg::Curve([5.5, 5.0, 6.1, 3.2, 7.3, 1.8]),
    Seg::Close,
];

const CORE: [Seg; 6] = [
    Seg::Move(11.1, 10.0),
    Seg::Curve([11.1, 11.712, 9.712, 13.1, 8.0, 13.1]),
    Seg::Curve([6.288, 13.1, 4.9, 11.712, 4.9, 10.0]),
    Seg::Curve([4.9, 8.288, 6.288, 6.9, 8.0, 6.9]),
    Seg::Curve([9.712, 6.9, 11.1, 8.288, 11.1, 10.0]),
    Seg::Close,
];

/// Two block eyes `width` by `height`, their tops at `top`.
const fn pair(top: f32, width: f32, height: f32) -> [Seg; 10] {
    let mut out = [Seg::Close; 10];
    let mut side = 0;
    while side < 2 {
        let left = [6.6, 9.4][side] - width / 2.0;
        let (right, bottom) = (left + width, top + height);
        out[side * 5] = Seg::Move(left, top);
        out[side * 5 + 1] = Seg::Line(right, top);
        out[side * 5 + 2] = Seg::Line(right, bottom);
        out[side * 5 + 3] = Seg::Line(left, bottom);
        side += 1;
    }
    out
}

const OPEN: [Seg; 10] = pair(9.0, 1.2, 2.2);
const WIDE: [Seg; 10] = pair(8.7, 1.35, 2.6);
const FOCUS: [Seg; 10] = pair(10.7, 1.5, 0.7);
const SAD: [Seg; 10] = pair(10.5, 1.5, 0.6);

const HAPPY: [Seg; 14] = [
    Seg::Move(5.55, 10.9),
    Seg::Line(6.6, 9.45),
    Seg::Line(7.65, 10.9),
    Seg::Line(7.05, 10.9),
    Seg::Line(6.6, 10.3),
    Seg::Line(6.15, 10.9),
    Seg::Close,
    Seg::Move(8.35, 10.9),
    Seg::Line(9.4, 9.45),
    Seg::Line(10.45, 10.9),
    Seg::Line(9.85, 10.9),
    Seg::Line(9.4, 10.3),
    Seg::Line(8.95, 10.9),
    Seg::Close,
];

const CLOSED: [Seg; 8] = [
    Seg::Move(5.7, 9.9),
    Seg::Curve([6.2, 10.75, 7.0, 10.75, 7.5, 9.9]),
    Seg::Curve([7.0, 11.35, 6.2, 11.35, 5.7, 9.9]),
    Seg::Close,
    Seg::Move(8.5, 9.9),
    Seg::Curve([9.0, 10.75, 9.8, 10.75, 10.3, 9.9]),
    Seg::Curve([9.8, 11.35, 9.0, 11.35, 8.5, 9.9]),
    Seg::Close,
];

/// Will's eyes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Eyes {
    Open,
    Wide,
    Focus,
    Sad,
    Happy,
    Closed,
}

impl Eyes {
    fn segs(self) -> &'static [Seg] {
        match self {
            Self::Open => &OPEN,
            Self::Wide => &WIDE,
            Self::Focus => &FOCUS,
            Self::Sad => &SAD,
            Self::Happy => &HAPPY,
            Self::Closed => &CLOSED,
        }
    }

    /// The row a blink squeezes the eyes towards.
    const fn middle(self) -> f32 {
        match self {
            Self::Open => 10.1,
            Self::Wide => 10.0,
            Self::Focus => 11.05,
            Self::Sad => 10.8,
            Self::Happy => 10.2,
            Self::Closed => 10.4,
        }
    }
}

/// Everything that sets one drawn frame of Will.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) scale: (f32, f32),
    pub(super) tilt: f32,
    pub(super) lift: f32,
    pub(super) fade: f32,
    pub(super) eyes: Eyes,
    pub(super) look: (f32, f32),
    pub(super) blink: f32,
}

/// Paints `frame` into `area` with the edge, core and eye colours.
pub(super) fn draw(
    scene: &mut SceneBuilder,
    area: Rect,
    frame: &Frame,
    colors: [Rgba8; 3],
    theme: &Theme,
) {
    let (sin, cos) = frame.tilt.to_radians().sin_cos();
    let (sx, sy) = frame.scale;
    let place = move |x: f32, y: f32| {
        let (dx, dy) = ((x - BASE.0) * sx, (y - BASE.1) * sy);
        (
            BASE.0 + dx * cos - dy * sin,
            BASE.1 + dx * sin + dy * cos + frame.lift,
        )
    };
    let middle = frame.eyes.middle();
    let (lx, ly) = frame.look;
    let gaze = move |x: f32, y: f32| place(x + lx, middle + (y - middle) * frame.blink + ly);
    let [edge, core, ink] = colors.map(|color| Rgba8 {
        a: (f32::from(color.a) * frame.fade.clamp(0.0, 1.0)).round() as u8,
        ..color
    });
    outline(&FLAME, &place, edge).draw(scene, area, theme);
    outline(&CORE, &place, core).draw(scene, area, theme);
    outline(frame.eyes.segs(), &gaze, ink).draw(scene, area, theme);
}

fn outline(segs: &[Seg], place: &impl Fn(f32, f32) -> (f32, f32), color: Rgba8) -> PathNode {
    segs.iter()
        .fold(sigil::widgets::path(16, 16), |node, seg| match *seg {
            Seg::Move(x, y) => {
                let (x, y) = place(x, y);
                node.move_to(x, y)
            }
            Seg::Line(x, y) => {
                let (x, y) = place(x, y);
                node.line_to(x, y)
            }
            Seg::Curve([ax, ay, bx, by, x, y]) => {
                let ((ax, ay), (bx, by), (x, y)) = (place(ax, ay), place(bx, by), place(x, y));
                node.cubic_to(ax, ay, bx, by, x, y)
            }
            Seg::Close => node.close(),
        })
        .fill(color)
}
