/// One step of an outline on the 120 grid.
#[derive(Clone, Copy)]
pub(super) enum Seg {
    Move(f32, f32),
    Line(f32, f32),
    Quad([f32; 4]),
    Curve([f32; 6]),
    Close,
}

use Seg::{Close, Curve, Line, Move, Quad};

/// The point Will scales and leans about: the bottom of his body.
pub(super) const BASE: (f32, f32) = (60.0, 98.0);
/// The centre of the glow behind him and its radius.
pub(super) const GLOW: (f32, f32, f32) = (60.0, 68.0, 60.0);
/// The centre of the summon ring and its two radii.
pub(super) const RING: (f32, f32, f32, f32) = (60.0, 106.0, 36.0, 6.0);
/// The centre of the status dot and its radius.
pub(super) const SIG: (f32, f32, f32) = (98.0, 22.0, 5.0);
/// Where the doze mote starts, its radius, and how far it drifts.
pub(super) const MOTE: (f32, f32, f32, (f32, f32)) = (78.0, 44.0, 1.6, (8.0, -22.0));
/// Where the burst sparks start: the centre of a 5-unit square.
pub(super) const SPARK: (f32, f32, f32) = (60.0, 66.0, 5.0);

/// The low crown: a round body with a short flick.
pub(super) const BODY: [Seg; 9] = [
    Move(55.0, 20.0),
    Curve([58.0, 32.0, 70.0, 36.0, 80.0, 44.0]),
    Curve([87.0, 50.0, 90.0, 58.0, 90.0, 68.0]),
    Curve([90.0, 84.57, 76.57, 98.0, 60.0, 98.0]),
    Curve([43.43, 98.0, 30.0, 84.57, 30.0, 68.0]),
    Curve([30.0, 58.0, 33.0, 50.0, 38.0, 44.0]),
    Curve([40.0, 49.0, 44.0, 51.0, 48.0, 50.0]),
    Curve([46.0, 40.0, 48.0, 30.0, 55.0, 20.0]),
    Close,
];

/// Each ember: start x and y, radius, seconds per rise, seconds of delay, and drift.
pub(super) const EMBERS: [(f32, f32, f32, f32, f32, f32); 6] = [
    (56.0, 34.0, 1.5, 2.2, 0.0, -6.0),
    (62.0, 30.0, 1.2, 1.8, 0.5, 5.0),
    (50.0, 40.0, 1.0, 2.6, 1.1, -10.0),
    (67.0, 38.0, 1.4, 2.0, 1.6, 9.0),
    (58.0, 26.0, 1.0, 1.6, 0.9, 2.0),
    (53.0, 44.0, 1.2, 2.4, 2.0, -3.0),
];
/// How far each burst spark flies.
pub(super) const SPARKS: [(f32, f32); 6] = [
    (0.0, -22.0),
    (19.0, -11.0),
    (19.0, 11.0),
    (0.0, 20.0),
    (-19.0, 11.0),
    (-19.0, -11.0),
];
/// How far an ember rises over one pass.
pub(super) const RISE: f32 = 40.0;

const K: f32 = 0.552_285;

/// A box with fully round ends, as SVG draws a rect with `rx = min(w, h) / 2`.
pub(super) const fn pill(x: f32, y: f32, w: f32, h: f32) -> [Seg; 10] {
    let r = if w < h { w / 2.0 } else { h / 2.0 };
    let k = r * K;
    let (right, bottom) = (x + w, y + h);
    [
        Move(x + r, y),
        Line(right - r, y),
        Curve([right - r + k, y, right, y + r - k, right, y + r]),
        Line(right, bottom - r),
        Curve([
            right,
            bottom - r + k,
            right - r + k,
            bottom,
            right - r,
            bottom,
        ]),
        Line(x + r, bottom),
        Curve([x + r - k, bottom, x, bottom - r + k, x, bottom - r]),
        Line(x, y + r),
        Curve([x, y + r - k, x + r - k, y, x + r, y]),
        Close,
    ]
}

/// An ellipse about `cx, cy`.
pub(super) const fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> [Seg; 6] {
    let (kx, ky) = (rx * K, ry * K);
    [
        Move(cx + rx, cy),
        Curve([cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry]),
        Curve([cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy]),
        Curve([cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry]),
        Curve([cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy]),
        Close,
    ]
}

const fn two(left: [Seg; 10], right: [Seg; 10]) -> [Seg; 20] {
    let mut out = [Close; 20];
    let mut at = 0;
    while at < 10 {
        out[at] = left[at];
        out[at + 10] = right[at];
        at += 1;
    }
    out
}

const OPEN: [Seg; 20] = two(pill(47.5, 70.0, 7.0, 14.0), pill(65.5, 70.0, 7.0, 14.0));
const WIDE: [Seg; 20] = two(pill(47.0, 67.5, 8.0, 17.0), pill(65.0, 67.5, 8.0, 17.0));
const FOCUS: [Seg; 20] = two(pill(47.5, 74.0, 7.5, 6.0), pill(65.0, 74.0, 7.5, 6.0));
const SAD: [Seg; 20] = two(pill(47.0, 79.0, 8.0, 4.5), pill(65.0, 79.0, 8.0, 4.5));
const PUZZLED: [Seg; 20] = two(pill(47.5, 70.0, 7.0, 14.0), pill(65.5, 74.0, 7.0, 7.0));
const HAPPY: [Seg; 6] = [
    Move(46.0, 79.0),
    Line(51.0, 72.5),
    Line(56.0, 79.0),
    Move(64.0, 79.0),
    Line(69.0, 72.5),
    Line(74.0, 79.0),
];
const CLOSED: [Seg; 4] = [
    Move(46.0, 77.0),
    Quad([51.0, 82.0, 56.0, 77.0]),
    Move(64.0, 77.0),
    Quad([69.0, 82.0, 74.0, 77.0]),
];

/// The width of a line eye's stroke.
pub(super) const STROKE: f32 = 3.4;

/// One part of a face: filled pills, or a stroked line.
pub(super) enum Mark {
    Fill(&'static [Seg]),
    Line(&'static [Seg]),
}

/// Will's eyes: the faces his states use from the sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Face {
    Open,
    Wide,
    Focus,
    Sad,
    Puzzled,
    Happy,
    Closed,
}

impl Face {
    /// The parts that draw this face.
    pub(super) const fn marks(self) -> &'static [Mark] {
        match self {
            Self::Open => &[Mark::Fill(&OPEN)],
            Self::Wide => &[Mark::Fill(&WIDE)],
            Self::Focus => &[Mark::Fill(&FOCUS)],
            Self::Sad => &[Mark::Fill(&SAD)],
            Self::Puzzled => &[Mark::Fill(&PUZZLED)],
            Self::Happy => &[Mark::Line(&HAPPY)],
            Self::Closed => &[Mark::Line(&CLOSED)],
        }
    }

    /// The row a blink squeezes this face towards: the middle of its box.
    pub(super) const fn middle(self) -> f32 {
        match self {
            Self::Open | Self::Focus | Self::Puzzled => 77.0,
            Self::Wide => 76.0,
            Self::Sad => 81.25,
            Self::Happy => 75.75,
            Self::Closed => 78.25,
        }
    }
}
