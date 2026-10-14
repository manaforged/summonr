use std::collections::BTreeMap;

use super::galaxy::{Field, field};
use super::stars::bright;
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

const WARM: Rgba8 = rgb(214, 176, 122);

const FLARE: Rgba8 = rgb(244, 246, 255);

const SKY_PERIOD: u64 = 50;

const METEOR_CYCLE: u64 = 440;

const METEOR_FLIGHT: u64 = 48;

const TRAIL: u16 = 20;

const TRAIL_STEP: f32 = 0.62;

pub(super) const DOT: [u8; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

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
    for star in super::stars::still(cols, rows, seed) {
        let cell = cell(star.text, star.fg, star.bg, false);
        set(&mut cells, cols, star.x, star.y, cell);
    }
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
    let mut spots = twinkling(cols, rows, beat, seed);
    sparkle(&mut spots, cols, rows, beat.max(0.0) as u64, seed);
    meteor(&mut spots, cols, rows, beat, seed);
    spots
}

fn twinkling(cols: u16, rows: u16, beat: f64, seed: u64) -> Vec<Spot> {
    super::stars::twinkling(cols, rows, beat, seed)
        .into_iter()
        .map(|star| spot(star.x, star.y, star.text, star.fg, star.bg, false))
        .collect()
}

fn sparkle(spots: &mut Vec<Spot>, cols: u16, rows: u16, ticks: u64, seed: u64) {
    if !(4..20).contains(&(ticks % SKY_PERIOD)) || cols == 0 || rows == 0 {
        return;
    }
    let cycle = (ticks / SKY_PERIOD) as u16;
    let pick = hash(cycle, 0x0101 ^ seed as u16);
    let bright = bright(cols, rows, seed);
    let (x, y) = match bright.get(pick as usize % bright.len().max(1)) {
        Some(at) => *at,
        None => (
            (pick % u32::from(cols)) as u16,
            (hash(cycle, 0x0202 ^ seed as u16) % u32::from(rows)) as u16,
        ),
    };
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

fn inside(cols: u16, rows: u16, x: f32, y: f32) -> Option<(u16, u16)> {
    (x >= 0.0 && y >= 0.0 && x < f32::from(cols) && y < f32::from(rows))
        .then_some((x as u16, y as u16))
}

pub(super) fn braille(bits: u8) -> &'static str {
    const DOTS: &str = "⠀⠁⠂⠃⠄⠅⠆⠇⠈⠉⠊⠋⠌⠍⠎⠏⠐⠑⠒⠓⠔⠕⠖⠗⠘⠙⠚⠛⠜⠝⠞⠟⠠⠡⠢⠣⠤⠥⠦⠧⠨⠩⠪⠫⠬⠭⠮⠯⠰⠱⠲⠳⠴⠵⠶⠷⠸⠹⠺⠻⠼⠽⠾⠿⡀⡁⡂⡃⡄⡅⡆⡇⡈⡉⡊⡋⡌⡍⡎⡏⡐⡑⡒⡓⡔⡕⡖⡗⡘⡙⡚⡛⡜⡝⡞⡟⡠⡡⡢⡣⡤⡥⡦⡧⡨⡩⡪⡫⡬⡭⡮⡯⡰⡱⡲⡳⡴⡵⡶⡷⡸⡹⡺⡻⡼⡽⡾⡿⢀⢁⢂⢃⢄⢅⢆⢇⢈⢉⢊⢋⢌⢍⢎⢏⢐⢑⢒⢓⢔⢕⢖⢗⢘⢙⢚⢛⢜⢝⢞⢟⢠⢡⢢⢣⢤⢥⢦⢧⢨⢩⢪⢫⢬⢭⢮⢯⢰⢱⢲⢳⢴⢵⢶⢷⢸⢹⢺⢻⢼⢽⢾⢿⣀⣁⣂⣃⣄⣅⣆⣇⣈⣉⣊⣋⣌⣍⣎⣏⣐⣑⣒⣓⣔⣕⣖⣗⣘⣙⣚⣛⣜⣝⣞⣟⣠⣡⣢⣣⣤⣥⣦⣧⣨⣩⣪⣫⣬⣭⣮⣯⣰⣱⣲⣳⣴⣵⣶⣷⣸⣹⣺⣻⣼⣽⣾⣿";
    let at = usize::from(bits) * 3;
    &DOTS[at..at + 3]
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

pub(super) fn under(haze: &Field) -> Rgba8 {
    blend(haze.top, haze.bottom, 0.5)
}

pub(super) fn index(cols: u16, x: u16, y: u16) -> usize {
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
