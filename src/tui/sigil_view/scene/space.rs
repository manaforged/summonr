use std::collections::BTreeMap;

use super::galaxy::{Field, field};
use super::*;

pub(in crate::tui::sigil_view) const SCRIM: Rgba8 = rgb(10, 8, 20);

pub(in crate::tui::sigil_view) const VOID: Rgba8 = rgb(5, 5, 10);

pub(in crate::tui::sigil_view) const PLATE: Rgba8 = rgb(42, 44, 58);

pub(in crate::tui::sigil_view) const STAR_DIM: Rgba8 = rgb(58, 74, 110);

pub(in crate::tui::sigil_view) const STAR_BRIGHT: Rgba8 = rgb(176, 200, 240);

#[derive(Clone)]
pub(in crate::tui::sigil_view) struct Spot {
    pub(in crate::tui::sigil_view) x: u16,
    pub(in crate::tui::sigil_view) y: u16,
    pub(in crate::tui::sigil_view) cell: Cell,
}

const FLYING: f32 = 1.5;

const WARM: Rgba8 = rgb(214, 176, 122);

const FLARE: Rgba8 = rgb(244, 246, 255);

const STAR_AREA: u32 = 60;

const SPEED: [f32; 3] = [0.08, 0.2, 0.45];

const LIGHT: [f32; 3] = [0.52, 0.8, 1.0];

const TEMPERATURE: [Rgba8; 5] = [
    rgb(255, 226, 190),
    rgb(196, 214, 255),
    rgb(236, 238, 248),
    rgb(236, 238, 248),
    rgb(220, 228, 255),
];

const SLOPE: f32 = 0.18;

const TWINKLE_EVERY: u32 = 8;

const TWINKLE_RATE: f64 = 0.09;

const TWINKLE: [f32; 3] = [0.45, 0.72, 1.0];

const FLY_SEED: u16 = 0x51f7;

const SKY_PERIOD: u64 = 50;

const METEOR_CYCLE: u64 = 440;

const METEOR_FLIGHT: u64 = 48;

const TRAIL: u16 = 20;

const TRAIL_STEP: f32 = 0.62;

const DOT: [u8; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

const LANES: [[f32; 4]; 6] = [
    [-0.08, 0.02, 1.08, 0.42],
    [-0.08, 0.48, 1.05, 0.78],
    [-0.08, 0.82, 0.72, 1.08],
    [1.08, 0.08, -0.08, 0.62],
    [0.15, -0.12, 0.78, 1.08],
    [0.88, 1.12, 0.22, -0.08],
];

struct Flight {
    x: f32,
    y: f32,
    ux: f32,
    uy: f32,
}

#[cfg(test)]
pub(in crate::tui::sigil_view) fn space(cols: u16, rows: u16, ticks: u64, seed: u64) -> Vec<Cell> {
    let mut cells = backdrop(cols, rows, seed);
    for spot in moving(cols, rows, ticks as f64, seed) {
        set(&mut cells, cols, spot.x, spot.y, spot.cell);
    }
    cells
}

#[cfg(test)]
pub(in crate::tui::sigil_view) fn backdrop(cols: u16, rows: u16, seed: u64) -> Vec<Cell> {
    field(cols, rows, seed)
        .iter()
        .map(|haze| {
            let text = if haze.top == haze.bottom { " " } else { "▀" };
            cell(text, haze.top, haze.bottom, false)
        })
        .collect()
}

pub(in crate::tui::sigil_view) fn moving(cols: u16, rows: u16, beat: f64, seed: u64) -> Vec<Spot> {
    let mut spots = flying(cols, rows, beat, seed);
    sparkle(&mut spots, cols, rows, beat.max(0.0) as u64, seed);
    meteor(&mut spots, cols, rows, beat, seed);
    spots
}

fn flying(cols: u16, rows: u16, beat: f64, seed: u64) -> Vec<Spot> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let field = field(cols, rows, seed);
    let wide = f64::from(cols) * 2.0;
    let tall = f64::from(rows) * 4.0;
    let count = ((u32::from(cols) * u32::from(rows) / STAR_AREA).max(8) as f32 * FLYING) as u32;
    let mut lit: BTreeMap<(u16, u16), (u8, Rgba8, usize)> = BTreeMap::new();
    for star in 0..count {
        let seed = hash(
            star as u16 ^ seed as u16,
            (star >> 16) as u16 ^ FLY_SEED ^ (seed >> 16) as u16,
        );
        let depth = match (seed >> 28) % 8 {
            0..=4 => 0,
            5 | 6 => 1,
            _ => 2,
        };
        let travel = f64::from(SPEED[depth]) * beat;
        let x = (f64::from(seed % 10_000) / 10_000.0 * wide - travel).rem_euclid(wide);
        let y = (f64::from((seed >> 14) % 10_000) / 10_000.0 * tall + travel * f64::from(SLOPE))
            .rem_euclid(tall);
        let colour = blend(
            VOID,
            TEMPERATURE[(seed >> 20) as usize % 5],
            LIGHT[depth] * twinkle(seed, beat),
        );
        let at = cell_of(x, y, cols, rows);
        let dot = DOT[(y as usize % 4) * 2 + x as usize % 2];
        let entry = lit.entry(at).or_insert((0, colour, depth));
        *entry = if depth >= entry.2 {
            (entry.0 | dot, colour, depth)
        } else {
            (entry.0 | dot, entry.1, entry.2)
        };
    }
    lit.into_iter()
        .map(|((x, y), (bits, colour, _))| {
            spot(
                x,
                y,
                braille(bits),
                colour,
                under(&field[index(cols, x, y)]),
                false,
            )
        })
        .collect()
}

fn twinkle(seed: u32, beat: f64) -> f32 {
    if !(seed >> 8).is_multiple_of(TWINKLE_EVERY) {
        return 1.0;
    }
    let phase = f64::from(seed % 997) / 997.0 * std::f64::consts::TAU;
    let wave = (beat * TWINKLE_RATE + phase).sin() * 0.5 + 0.5;
    TWINKLE[((wave * TWINKLE.len() as f64) as usize).min(TWINKLE.len() - 1)]
}

fn sparkle(spots: &mut Vec<Spot>, cols: u16, rows: u16, ticks: u64, seed: u64) {
    if !(4..20).contains(&(ticks % SKY_PERIOD)) || cols == 0 || rows == 0 {
        return;
    }
    let cycle = (ticks / SKY_PERIOD) as u16;
    let x = (hash(cycle, 0x0101 ^ seed as u16) % u32::from(cols)) as u16;
    let y = (hash(cycle, 0x0202 ^ seed as u16) % u32::from(rows)) as u16;
    let field = field(cols, rows, seed);
    spots.push(spot(
        x,
        y,
        "✦",
        STAR_BRIGHT,
        under(&field[index(cols, x, y)]),
        true,
    ));
}

fn meteor(spots: &mut Vec<Spot>, cols: u16, rows: u16, beat: f64, seed: u64) {
    let Some(flight) = flight(cols, rows, beat, seed) else {
        return;
    };
    let field = field(cols, rows, seed);
    let head = inside(cols, rows, flight.x, flight.y);
    let mut trail: BTreeMap<(u16, u16), (u8, Rgba8)> = BTreeMap::new();
    for step in (1..TRAIL).rev() {
        let along = f32::from(step) * TRAIL_STEP;
        let fade = 1.0 - f32::from(step) / f32::from(TRAIL);
        let fg = if step < 4 {
            blend(WARM, FLARE, fade)
        } else {
            blend(STAR_DIM, WARM, fade)
        };
        let (x, y) = (flight.x - flight.ux * along, flight.y - flight.uy * along);
        let Some(at) = inside(cols, rows, x, y).filter(|at| Some(*at) != head) else {
            continue;
        };
        let dot = DOT[((y.fract() * 4.0) as usize).min(3) * 2 + usize::from(x.fract() >= 0.5)];
        let entry = trail.entry(at).or_insert((0, fg));
        *entry = (entry.0 | dot, fg);
    }
    for ((x, y), (bits, fg)) in trail {
        spots.push(spot(
            x,
            y,
            braille(bits),
            fg,
            under(&field[index(cols, x, y)]),
            false,
        ));
    }
    if let Some((x, y)) = head {
        spots.push(spot(
            x,
            y,
            "✦",
            FLARE,
            under(&field[index(cols, x, y)]),
            true,
        ));
    }
}

fn flight(cols: u16, rows: u16, beat: f64, seed: u64) -> Option<Flight> {
    let phase = (beat.max(0.0) % METEOR_CYCLE as f64) as f32;
    if phase >= METEOR_FLIGHT as f32 || cols == 0 || rows == 0 {
        return None;
    }
    let t = phase / METEOR_FLIGHT as f32;
    let lane =
        LANES[((beat.max(0.0) / METEOR_CYCLE as f64) as usize + seed as usize) % LANES.len()];
    let dx = (lane[2] - lane[0]) * f32::from(cols);
    let dy = (lane[3] - lane[1]) * f32::from(rows);
    let span = (dx * dx + dy * dy).sqrt().max(1.0);
    Some(Flight {
        x: lerp(lane[0], lane[2], t) * f32::from(cols),
        y: lerp(lane[1], lane[3], t) * f32::from(rows),
        ux: dx / span,
        uy: dy / span,
    })
}

fn cell_of(x: f64, y: f64, cols: u16, rows: u16) -> (u16, u16) {
    (
        ((x / 2.0) as u16).min(cols.saturating_sub(1)),
        ((y / 4.0) as u16).min(rows.saturating_sub(1)),
    )
}

fn inside(cols: u16, rows: u16, x: f32, y: f32) -> Option<(u16, u16)> {
    (x >= 0.0 && y >= 0.0 && x < f32::from(cols) && y < f32::from(rows))
        .then_some((x as u16, y as u16))
}

fn braille(bits: u8) -> &'static str {
    const DOTS: &str = "⠀⠁⠂⠃⠄⠅⠆⠇⠈⠉⠊⠋⠌⠍⠎⠏⠐⠑⠒⠓⠔⠕⠖⠗⠘⠙⠚⠛⠜⠝⠞⠟⠠⠡⠢⠣⠤⠥⠦⠧⠨⠩⠪⠫⠬⠭⠮⠯⠰⠱⠲⠳⠴⠵⠶⠷⠸⠹⠺⠻⠼⠽⠾⠿⡀⡁⡂⡃⡄⡅⡆⡇⡈⡉⡊⡋⡌⡍⡎⡏⡐⡑⡒⡓⡔⡕⡖⡗⡘⡙⡚⡛⡜⡝⡞⡟⡠⡡⡢⡣⡤⡥⡦⡧⡨⡩⡪⡫⡬⡭⡮⡯⡰⡱⡲⡳⡴⡵⡶⡷⡸⡹⡺⡻⡼⡽⡾⡿⢀⢁⢂⢃⢄⢅⢆⢇⢈⢉⢊⢋⢌⢍⢎⢏⢐⢑⢒⢓⢔⢕⢖⢗⢘⢙⢚⢛⢜⢝⢞⢟⢠⢡⢢⢣⢤⢥⢦⢧⢨⢩⢪⢫⢬⢭⢮⢯⢰⢱⢲⢳⢴⢵⢶⢷⢸⢹⢺⢻⢼⢽⢾⢿⣀⣁⣂⣃⣄⣅⣆⣇⣈⣉⣊⣋⣌⣍⣎⣏⣐⣑⣒⣓⣔⣕⣖⣗⣘⣙⣚⣛⣜⣝⣞⣟⣠⣡⣢⣣⣤⣥⣦⣧⣨⣩⣪⣫⣬⣭⣮⣯⣰⣱⣲⣳⣴⣵⣶⣷⣸⣹⣺⣻⣼⣽⣾⣿";
    let at = usize::from(bits) * 3;
    &DOTS[at..at + 3]
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn under(haze: &Field) -> Rgba8 {
    blend(haze.top, haze.bottom, 0.5)
}

fn index(cols: u16, x: u16, y: u16) -> usize {
    usize::from(y) * usize::from(cols) + usize::from(x)
}

fn spot(x: u16, y: u16, text: &'static str, fg: Rgba8, bg: Rgba8, bold: bool) -> Spot {
    Spot {
        x,
        y,
        cell: cell(text, fg, bg, bold),
    }
}

fn cell(text: &'static str, fg: Rgba8, bg: Rgba8, bold: bool) -> Cell {
    Cell {
        text: text.into(),
        fg: ink(fg),
        bg: ink(bg),
        bold,
        italic: false,
        ..Cell::blank()
    }
}

pub(in crate::tui::sigil_view) fn hash(x: u16, y: u16) -> u32 {
    let mut n = u32::from(x).wrapping_mul(0x9E37_79B1) ^ u32::from(y).wrapping_mul(0x85EB_CA77);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7FEB_352D);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846C_A68B);
    n ^= n >> 16;
    n
}

#[cfg(test)]
#[path = "space_tests.rs"]
mod tests;

pub(in crate::tui::sigil_view) fn set(cells: &mut [Cell], cols: u16, x: u16, y: u16, cell: Cell) {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(slot) = cells.get_mut(index) {
        *slot = cell;
    }
}
