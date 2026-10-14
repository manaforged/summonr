use std::cell::RefCell;
use std::rc::Rc;

use sigil::geometry::{Point, Rect};
use sigil::scene::{SceneBuilder, SceneRectStyle};

use super::*;

const BAND: Rgba8 = rgb(58, 56, 96);

const STARLIGHT: Rgba8 = rgb(112, 120, 168);

const ROSE: Rgba8 = rgb(98, 46, 80);

const GOLD: Rgba8 = rgb(150, 120, 86);

const ZONES: [[Rgba8; 2]; 3] = [
    [rgb(40, 64, 100), rgb(104, 140, 178)],
    [BAND, STARLIGHT],
    [rgb(80, 50, 98), rgb(156, 116, 172)],
];

const HUE: [f32; 3] = [0.7, 0.5, 0.04];

const LEVELS: f32 = 8.0;

const WANDER: f32 = 0.14;

const WIDTH: f32 = 0.42;

const STRETCH: [f32; 2] = [1.2, 2.2];

const WARP: [f32; 2] = [0.8, 1.1];

const RAGGED: f32 = 0.1;

const GRAIN: [f32; 3] = [9.0, 7.0, 0.24];

const BULGE: [f32; 2] = [0.5, 0.24];

const LANE: [f32; 3] = [0.22, 0.025, 0.03];

const FILAMENT: [f32; 2] = [0.4, 0.55];

const NEBULA: f32 = 0.45;

#[derive(Clone, Copy)]
pub(super) struct Field {
    pub(super) top: Rgba8,
    pub(super) bottom: Rgba8,
}

type Cached = ((u16, u16, u64), Rc<[Field]>);

thread_local! {
    static FIELD: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

pub(super) fn field(cols: u16, rows: u16, seed: u64) -> Rc<[Field]> {
    let key = (cols, rows, seed);
    FIELD.with_borrow_mut(|cached| {
        if let Some((held, field)) = cached.as_ref()
            && *held == key
        {
            return field.clone();
        }
        let field: Rc<[Field]> = draw(cols, rows, &Shape::of(seed)).into();
        *cached = Some((key, field.clone()));
        field
    })
}

struct Shape {
    sin: f32,
    cos: f32,
    shift: f32,
    core: f32,
    bend: f32,
    width: f32,
    offset: f32,
}

impl Shape {
    fn of(seed: u64) -> Self {
        let roll = |salt: u16| {
            let mixed = super::space::hash(
                seed as u16 ^ salt,
                (seed >> 16) as u16 ^ salt.rotate_left(7),
            );
            f32::from((mixed % 10_000) as u16) / 10_000.0
        };
        let flip = if roll(2) > 0.5 {
            std::f32::consts::PI
        } else {
            0.0
        };
        let (sin, cos) = ((roll(1) - 0.5) * 1.8 + flip).sin_cos();
        Self {
            sin,
            cos,
            shift: (roll(3) - 0.5) * 0.6,
            core: (roll(4) - 0.5) * 1.8,
            bend: (roll(5) - 0.5) * 0.5,
            width: 0.8 + 0.4 * roll(6),
            offset: roll(7) * 97.0,
        }
    }
}

struct Sample {
    shade: f32,
    glow: f32,
    nebula: f32,
    hue: f32,
}

fn draw(cols: u16, rows: u16, shape: &Shape) -> Vec<Field> {
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let upper = sample(cols, rows, f32::from(x), f32::from(y) - 0.25, shape);
            let lower = sample(cols, rows, f32::from(x), f32::from(y) + 0.25, shape);
            cells.push(Field {
                top: haze(&upper),
                bottom: haze(&lower),
            });
        }
    }
    cells
}

fn sample(cols: u16, rows: u16, x: f32, y: f32, shape: &Shape) -> Sample {
    let half = f32::from(rows.max(1)) / 2.0;
    let px = (x + 0.5 - f32::from(cols) / 2.0) * 0.5 / half;
    let py = (y + 0.5 - half) / half;
    let along = px * shape.cos + py * shape.sin;
    let drift = WANDER * (fbm(along * 0.8 + shape.offset, 1.7, 3) - 0.5) * 2.0;
    let across = py * shape.cos - px * shape.sin - drift - shape.shift - shape.bend * along * along;
    let (u, v) = (along * STRETCH[0] + shape.offset, across * STRETCH[1]);
    let (wx, wy) = (
        u + WARP[1] * (fbm(u * WARP[0] + 3.0, v * WARP[0] + 7.0, 3) - 0.5),
        v + WARP[1] * (fbm(u * WARP[0] - 5.0, v * WARP[0] + 1.0, 3) - 0.5),
    );
    let width = WIDTH * shape.width * (0.65 + 0.7 * fbm(u * 0.7 + 9.0, 2.0, 3));
    let ragged = across + RAGGED * (fbm(wx * 3.0 + 70.0, wy * 3.0, 4) - 0.5);
    let profile = gauss(ragged / width);
    let clouds = ((fbm(wx * 1.7, wy * 2.4, 5) - 0.3) * 1.8).clamp(0.0, 1.0);
    let grain = 1.0 - GRAIN[2] / 2.0 + GRAIN[2] * noise(wx * GRAIN[0] + 5.0, wy * GRAIN[1]);
    let stars = profile * (0.38 + 0.72 * clouds) * grain + gauss(across / 0.9) * 0.05;
    let (bu, bv) = ((along - shape.core) / BULGE[0], across / BULGE[1]);
    let edge = 0.45 + 1.1 * fbm(wx * 2.2 + 40.0, wy * 2.2, 4);
    let bulge = (-(bu * bu + bv * bv) / edge).exp();
    let dust = dust(along, across, width, (u, wx, wy), bulge);
    let light = (stars * 0.85 + bulge * 0.95).min(1.0) * 1.15;
    let nebula = ((fbm(wx * 2.6 + 50.0, wy * 3.4, 4) - 0.5) * 4.0).clamp(0.0, 1.0);
    Sample {
        shade: light * (1.0 - 0.8 * dust),
        glow: bulge * (1.0 - 0.6 * dust),
        nebula: nebula * profile * (0.45 + dust),
        hue: fbm(wx * HUE[0] + 90.0, wy * HUE[1], 3),
    }
}

fn dust(along: f32, across: f32, width: f32, (u, x, y): (f32, f32, f32), bulge: f32) -> f32 {
    let wobble = 0.04 * (along * 2.3).sin() + 0.05 * (fbm(u * 1.5 + 60.0, 1.0, 3) - 0.5);
    let breadth = width * LANE[0] + LANE[1] + LANE[2] * fbm(x * 2.0 + 80.0, y, 3);
    let broken = ((fbm(x * 0.9 + 11.0, y * 2.4, 5) - 0.34) * 2.4).clamp(0.0, 1.0);
    let rift = gauss((across - 0.02 - wobble) / breadth) * broken * (1.0 + 0.5 * bulge);
    let ridge = ((ridged(x * 2.0 + 30.0, y * 2.0 - 4.0) - FILAMENT[0]) * 1.6).clamp(0.0, 1.0);
    let filament = ridge * gauss(across / (width * 1.4)) * FILAMENT[1];
    (rift * 0.85).max(filament).min(1.0)
}

fn haze(sample: &Sample) -> Rgba8 {
    let shade = ((0.72 * sample.shade).clamp(0.0, 1.0) * LEVELS).round() / LEVELS;
    let warm = ((sample.glow * 1.6).clamp(0.0, 1.0) * 2.0).round() / 2.0;
    let ramp = ((shade - 0.375) * 1.6).clamp(0.0, 1.0);
    let zone = match sample.hue - 0.5 {
        _ if shade <= 0.25 => 1,
        offset if offset < -HUE[2] => 0,
        offset if offset > HUE[2] => 2,
        _ => 1,
    };
    let tint = if warm > 0.0 {
        blend(
            blend(BAND, STARLIGHT, ramp),
            GOLD,
            warm * (shade * 1.5).min(1.0),
        )
    } else if sample.nebula > NEBULA {
        blend(BAND, ROSE, 0.75)
    } else {
        blend(ZONES[zone][0], ZONES[zone][1], ramp)
    };
    blend(VOID, tint, shade)
}

fn gauss(x: f32) -> f32 {
    (-x * x).exp()
}

fn fbm(mut x: f32, mut y: f32, octaves: u8) -> f32 {
    let (mut sum, mut weight, mut total) = (0.0, 1.0, 0.0);
    for octave in 0..octaves {
        let at = f32::from(octave);
        sum += noise(x + at * 17.3, y - at * 9.1) * weight;
        total += weight;
        weight *= 0.5;
        (x, y) = (x * 2.03, y * 2.03);
    }
    sum / total
}

fn ridged(mut x: f32, mut y: f32) -> f32 {
    let (mut sum, mut weight, mut total, mut gain) = (0.0, 1.0, 0.0, 1.0);
    for octave in 0..4_u8 {
        let at = f32::from(octave);
        let ridge = 1.0 - (2.0 * noise(x + at * 23.1, y + at * 5.7) - 1.0).abs();
        let ridge = ridge * ridge * gain;
        gain = (ridge * 1.6).clamp(0.0, 1.0);
        sum += ridge * weight;
        total += weight;
        weight *= 0.55;
        (x, y) = (x * 2.1, y * 2.1);
    }
    sum / total
}

fn noise(x: f32, y: f32) -> f32 {
    let (cx, cy) = (x.floor(), y.floor());
    let (fx, fy) = (smooth(x - cx), smooth(y - cy));
    let corner = |dx: f32, dy: f32| {
        let hx = (cx + dx) as i32 as u16;
        let hy = (cy + dy) as i32 as u16;
        f32::from((super::space::hash(hx, hy) >> 16) as u16) / f32::from(u16::MAX)
    };
    let top = corner(0.0, 0.0) + (corner(1.0, 0.0) - corner(0.0, 0.0)) * fx;
    let bottom = corner(0.0, 1.0) + (corner(1.0, 1.0) - corner(0.0, 1.0)) * fx;
    top + (bottom - top) * fy
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

pub(in crate::tui::sigil_view) fn paint_sky(
    scene: &mut SceneBuilder,
    area: Rect,
    (cols, rows): (u16, u16),
    seed: u64,
) {
    let field = field(cols, rows, seed);
    let cell = metrics();
    for y in 0..rows {
        let top = area.origin.y + f32::from(y) * cell.h;
        let row = &field[usize::from(y) * usize::from(cols)
            ..usize::from(y) * usize::from(cols) + usize::from(cols)];
        let mut band = |pick: &dyn Fn(&Field) -> Option<Rgba8>, at: f32, height: f32| {
            let mut x = 0;
            while x < row.len() {
                let Some(colour) = pick(&row[x]) else {
                    x += 1;
                    continue;
                };
                let start = x;
                while x < row.len() && pick(&row[x]) == Some(colour) {
                    x += 1;
                }
                let left = area.origin.x + start as f32 * cell.w;
                let rect = Rect::new(Point::new(left, at), (x - start) as f32 * cell.w, height);
                scene.draw_rect(rect, SceneRectStyle::solid(colour));
            }
        };
        band(&|haze| Some(haze.top), top, cell.h);
        band(
            &|haze| (haze.bottom != haze.top).then_some(haze.bottom),
            top + cell.h / 2.0,
            cell.h / 2.0,
        );
    }
}
