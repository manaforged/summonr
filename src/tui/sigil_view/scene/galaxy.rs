use std::cell::RefCell;
use std::rc::Rc;

use super::*;

const HAZE: [Rgba8; 6] = [
    rgb(10, 8, 20),
    rgb(13, 11, 27),
    rgb(18, 15, 35),
    rgb(25, 20, 45),
    rgb(36, 28, 56),
    rgb(56, 42, 66),
];

const DUST: [Rgba8; 6] = [
    rgb(44, 48, 82),
    rgb(62, 64, 104),
    rgb(86, 84, 128),
    rgb(116, 108, 150),
    rgb(160, 140, 156),
    rgb(214, 184, 156),
];

const TILT: f32 = -0.48;

const WIDTH: f32 = 0.36;

const CORE: [f32; 3] = [0.9, 0.42, 0.24];

const RIFT: [f32; 3] = [0.04, 0.08, 0.8];

const FILAMENT: f32 = 0.06;

const SEED: u16 = 977;

pub(super) struct Field {
    pub(super) bg: Rgba8,
    pub(super) fg: Rgba8,
    pub(super) dots: u8,
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

fn draw(cols: u16, rows: u16, glow: f32) -> Vec<Field> {
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    let half = f32::from(rows.max(1)) / 2.0;
    let (sin, cos) = TILT.sin_cos();
    for y in 0..rows {
        for x in 0..cols {
            let px = (f32::from(x) + 0.5 - f32::from(cols) / 2.0) * 0.5 / half;
            let py = (f32::from(y) + 0.5 - half) / half;
            let along = px * cos + py * sin;
            let across = py * cos - px * sin;
            let shade = glow * light(along, across) * (1.0 - 0.7 * lanes(along, across, px, py));
            let level = level(shade);
            let density = shade * (0.3 + 0.7 * fbm(px * 3.0, py * 3.0));
            cells.push(Field {
                bg: HAZE[level],
                fg: DUST[level],
                dots: dots(x, y, density),
            });
        }
    }
    cells
}

fn light(along: f32, across: f32) -> f32 {
    let band = (-(across / WIDTH).powi(2)).exp() * (0.7 + 0.3 * (-(along / 1.6).powi(2)).exp());
    let core = (-((along - CORE[0]) / CORE[1]).powi(2) - (across / CORE[2]).powi(2)).exp();
    (band * 0.7 + core * 0.6).min(1.0)
}

fn lanes(along: f32, across: f32, px: f32, py: f32) -> f32 {
    let center = RIFT[0] + 0.05 * (along * 2.7 + 1.0).sin();
    let width = RIFT[1] * (0.6 + 0.8 * fbm(px * 2.0 + 11.0, py * 2.0));
    let rift = (-((across - center) / width).powi(2)).exp() * RIFT[2];
    let ridge = (fbm(px * 2.4 + 5.0, py * 2.4 - 3.0) - 0.5).abs();
    let filament = (1.0 - ridge / FILAMENT).max(0.0) * (-(across / WIDTH).powi(2)).exp();
    rift.max(filament * 0.85)
}

fn level(value: f32) -> usize {
    ((value * HAZE.len() as f32).max(0.0) as usize).min(HAZE.len() - 1)
}

fn dots(x: u16, y: u16, density: f32) -> u8 {
    let chance = (density.powf(1.5) * 1.25).min(0.92) * 1000.0;
    (0..8u16).fold(0, |bits, bit| {
        let roll = super::space::hash(x.wrapping_mul(8).wrapping_add(bit), y ^ SEED) % 1000;
        if (roll as f32) < chance {
            bits | 1 << bit
        } else {
            bits
        }
    })
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
