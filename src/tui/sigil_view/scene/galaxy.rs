use std::cell::RefCell;
use std::rc::Rc;

use super::*;

const HUES: [Rgba8; 3] = [rgb(30, 74, 98), rgb(78, 56, 118), rgb(116, 48, 92)];

const GOLD: Rgba8 = rgb(170, 132, 96);

const LIGHTS: [Rgba8; 3] = [rgb(150, 196, 226), rgb(186, 176, 236), rgb(236, 176, 206)];

const LEVELS: f32 = 8.0;

const WIDE: [f32; 2] = [1.1, 0.22];

const TILT: f32 = -0.48;

const WIDTH: f32 = 0.55;

const CORE: [f32; 3] = [0.9, 0.42, 0.24];

const RIFT: [f32; 3] = [0.04, 0.08, 0.8];

const FILAMENT: f32 = 0.06;

const DITHER: f32 = 0.9;

const DITHER_SEED: u16 = 0x2f1;

#[derive(Clone, Copy)]
pub(super) struct Field {
    pub(super) top: Rgba8,
    pub(super) bottom: Rgba8,
    pub(super) density: f32,
    pub(super) core: f32,
    pub(super) hue: f32,
}

type Cached = ((u16, u16, u32), Rc<[Field]>);

thread_local! {
    static FIELD: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

pub(super) fn field(cols: u16, rows: u16, glow: f32) -> Rc<[Field]> {
    let key = (cols, rows, glow.to_bits());
    FIELD.with_borrow_mut(|cached| {
        if let Some((held, field)) = cached.as_ref()
            && *held == key
        {
            return field.clone();
        }
        let field: Rc<[Field]> = draw(cols, rows, glow).into();
        *cached = Some((key, field.clone()));
        field
    })
}

struct Sample {
    shade: f32,
    clump: f32,
    core: f32,
    hue: f32,
}

fn draw(cols: u16, rows: u16, glow: f32) -> Vec<Field> {
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        for x in 0..cols {
            let upper = sample(cols, rows, f32::from(x), f32::from(y) - 0.25, glow);
            let lower = sample(cols, rows, f32::from(x), f32::from(y) + 0.25, glow);
            let row = y.wrapping_mul(2);
            cells.push(Field {
                top: haze(&upper, x, row),
                bottom: haze(&lower, x, row + 1),
                density: upper.shade * upper.clump,
                core: upper.core,
                hue: upper.hue,
            });
        }
    }
    cells
}

fn sample(cols: u16, rows: u16, x: f32, y: f32, glow: f32) -> Sample {
    let half = f32::from(rows.max(1)) / 2.0;
    let px = (x + 0.5 - f32::from(cols) / 2.0) * 0.5 / half;
    let py = (y + 0.5 - half) / half;
    let (sin, cos) = TILT.sin_cos();
    let along = px * cos + py * sin;
    let across = py * cos - px * sin;
    let band = (-(across / WIDTH).powi(2)).exp() * (0.7 + 0.3 * (-(along / 1.6).powi(2)).exp());
    let core = (-((along - CORE[0]) / CORE[1]).powi(2) - (across / CORE[2]).powi(2)).exp();
    let wide = (-(across / WIDE[0]).powi(2)).exp() * WIDE[1];
    let light = (band * 0.7 + wide + core * 0.6).min(1.0) * glow;
    Sample {
        shade: light * (1.0 - 0.75 * lanes(along, across, px, py)),
        clump: 0.35 + 0.65 * fbm(px * 3.0, py * 3.0),
        core,
        hue: ((fbm(px * 1.2 + 20.0, py * 1.2 - 7.0) - 0.3) / 0.4).clamp(0.0, 1.0),
    }
}

fn lanes(along: f32, across: f32, px: f32, py: f32) -> f32 {
    let center = RIFT[0] + 0.05 * (along * 2.7 + 1.0).sin();
    let width = RIFT[1] * (0.6 + 0.8 * fbm(px * 2.0 + 11.0, py * 2.0));
    let rift = (-((across - center) / width).powi(2)).exp() * RIFT[2];
    let ridge = (fbm(px * 2.4 + 5.0, py * 2.4 - 3.0) - 0.5).abs();
    let filament = (1.0 - ridge / FILAMENT).max(0.0) * (-(across / WIDTH).powi(2)).exp();
    rift.max(filament * 0.85)
}

fn haze(sample: &Sample, x: u16, row: u16) -> Rgba8 {
    let roll = super::space::hash(x, row ^ DITHER_SEED);
    let jitter = |shift: u32| ((roll >> shift) % 1000) as f32 / 1000.0 - 0.5;
    let shade = sample.shade * (0.75 + 0.25 * sample.clump);
    let level = (shade * LEVELS + jitter(0) * DITHER)
        .round()
        .clamp(0.0, LEVELS)
        / LEVELS;
    let hue = ((sample.hue * 4.0 + jitter(10) * DITHER)
        .round()
        .clamp(0.0, 4.0))
        / 4.0;
    let warm = (sample.core * 3.0 + jitter(20) * DITHER)
        .round()
        .clamp(0.0, 3.0)
        / 3.0;
    blend(SCRIM, blend(tint(&HUES, hue), GOLD, warm * 0.8), level)
}

pub(super) fn starlight(hue: f32) -> Rgba8 {
    tint(&LIGHTS, hue)
}

fn tint(colours: &[Rgba8; 3], hue: f32) -> Rgba8 {
    if hue < 0.5 {
        blend(colours[0], colours[1], hue * 2.0)
    } else {
        blend(colours[1], colours[2], hue * 2.0 - 1.0)
    }
}

fn fbm(x: f32, y: f32) -> f32 {
    (noise(x, y) * 4.0 + noise(x * 2.1, y * 2.1) * 2.0 + noise(x * 4.3, y * 4.3)) / 7.0
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
