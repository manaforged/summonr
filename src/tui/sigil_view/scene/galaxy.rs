use std::cell::RefCell;
use std::rc::Rc;

use sigil::geometry::{Point, Rect};
use sigil::scene::{SceneBuilder, SceneRectStyle};

use super::*;

const BAND: Rgba8 = rgb(58, 56, 96);

const LEVELS: f32 = 8.0;

const GOLD: Rgba8 = rgb(118, 96, 78);

const ROSE: Rgba8 = rgb(98, 46, 80);

const WIDE: [f32; 2] = [0.9, 0.05];

const WANDER: f32 = 0.14;

const WIDTH: f32 = 0.42;

const CORE: [f32; 2] = [0.42, 0.24];

const GLOW: f32 = 1.15;

const RIFT: [f32; 3] = [0.04, 0.1, 0.55];

const FILAMENT: f32 = 0.11;

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
    clump: f32,
    core: f32,
    nebula: f32,
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
    let (nx, ny, na) = (px + shape.offset, py - shape.offset, along + shape.offset);
    let drift = WANDER * (fbm(na * 0.8 + 3.0, 1.7) - 0.5) * 2.0;
    let across = py * shape.cos - px * shape.sin - drift - shape.shift - shape.bend * along * along;
    let width = WIDTH * shape.width * (0.6 + 0.8 * fbm(na * 1.1 + 7.0, 4.1));
    let patches = 0.35 + 0.65 * fbm(na * 1.7 + 11.0, across * 2.5 + 2.0);
    let fray = (0.6 + 0.8 * fbm(nx * 4.2 + 13.0, ny * 4.2)).min(1.0);
    let band = (-(across / width).powi(2)).exp() * patches * fray;
    let core = (-((along - shape.core) / CORE[0]).powi(2) - (across / CORE[1]).powi(2)).exp();
    let wide = (-(across / WIDE[0]).powi(2)).exp() * WIDE[1];
    let cloud = clouds(across, width, nx, ny);
    let light = (band * 0.8 + wide + core * 0.6 + cloud * 0.5).min(1.0) * GLOW;
    Sample {
        shade: light * (1.0 - 0.75 * lanes(na, across, width, nx, ny)),
        clump: 0.35 + 0.65 * fbm(nx * 2.2, ny * 2.2),
        core,
        nebula: (cloud / (band * 0.8 + cloud + 0.05)).min(1.0),
    }
}

fn clouds(across: f32, width: f32, x: f32, y: f32) -> f32 {
    let near = (-(across / (width * 1.5)).powi(2)).exp();
    let body = ((fbm(x * 1.8 + 31.0, y * 1.8 - 17.0) - 0.5) * 3.0).max(0.0);
    let glow = 0.6 + 0.6 * fbm(x * 3.5 + 3.0, y * 3.5 + 9.0);
    let dust = ((fbm(x * 2.6 + 40.0, y * 2.6) - 0.6) * 3.0).clamp(0.0, 1.0);
    (body * near * glow * (1.0 - 0.7 * dust)).min(1.0)
}

fn lanes(along: f32, across: f32, width: f32, px: f32, py: f32) -> f32 {
    let center = RIFT[0] + 0.05 * (along * 2.7 + 1.0).sin();
    let breadth = RIFT[1] * (0.6 + 0.8 * fbm(px * 2.0 + 11.0, py * 2.0));
    let broken = ((fbm(along * 2.2 + 5.0, 3.3) - 0.4) * 3.0).clamp(0.0, 1.0);
    let rift = (-((across - center) / breadth).powi(2)).exp() * RIFT[2] * broken;
    let ridge = (fbm(px * 2.4 + 5.0, py * 2.4 - 3.0) - 0.5).abs();
    let filament = (1.0 - ridge / FILAMENT).max(0.0) * (-(across / width).powi(2)).exp();
    rift.max(filament * 0.5)
}

fn haze(sample: &Sample) -> Rgba8 {
    let step = |value: f32, steps: f32| (value.clamp(0.0, 1.0) * steps).round() / steps;
    let shade = step(0.72 * sample.shade * (0.75 + 0.25 * sample.clump), LEVELS);
    let tint = blend(
        blend(BAND, ROSE, step(sample.nebula, 2.0) * 0.7),
        GOLD,
        step(sample.core, 3.0) * 0.75,
    );
    blend(VOID, tint, shade)
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
