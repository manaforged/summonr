use std::collections::BTreeMap;

use super::galaxy::{Field, field, starlight};
use super::*;

pub(in crate::tui::sigil_view) const SCRIM: Rgba8 = rgb(10, 8, 20);

pub(in crate::tui::sigil_view) const PLATE: Rgba8 = rgb(42, 44, 58);

pub(in crate::tui::sigil_view) const STAR_DIM: Rgba8 = rgb(58, 74, 110);

pub(in crate::tui::sigil_view) const STAR_BRIGHT: Rgba8 = rgb(176, 200, 240);

pub(in crate::tui::sigil_view) const QUIET: u8 = 0;

pub(in crate::tui::sigil_view) const HALO: u8 = 1;

pub(in crate::tui::sigil_view) const DUST: u8 = 2;

pub(in crate::tui::sigil_view) fn sky_name(sky: u8) -> &'static str {
    match sky % 3 {
        HALO => "halo",
        DUST => "dust",
        _ => "quiet",
    }
}

pub(in crate::tui::sigil_view) fn step_sky(sky: u8, step: i8) -> u8 {
    (i16::from(sky) + i16::from(step)).rem_euclid(3) as u8
}

pub(in crate::tui::sigil_view) struct Spot {
    pub(in crate::tui::sigil_view) x: u16,
    pub(in crate::tui::sigil_view) y: u16,
    pub(in crate::tui::sigil_view) cell: Cell,
}

struct Look {
    dust: f32,
    flying: f32,
    glow: f32,
    halo: bool,
}

fn look(sky: u8) -> Look {
    let dust = sky % 3 == DUST;
    Look {
        dust: if dust { 1.6 } else { 1.0 },
        flying: if dust { 1.5 } else { 1.0 },
        glow: if dust { 1.15 } else { 1.0 },
        halo: sky % 3 == HALO,
    }
}

const WARM: Rgba8 = rgb(214, 176, 122);

const FLARE: Rgba8 = rgb(244, 246, 255);

const DUST_BASE: f32 = 0.006;

const DUST_BAND: f32 = 0.45;

const STAR_AREA: u32 = 40;

const SPEED: [f32; 3] = [0.05, 0.12, 0.3];

const FLY: [Rgba8; 3] = [rgb(70, 82, 120), rgb(130, 148, 196), rgb(214, 224, 250)];

const SLOPE: f32 = 0.18;

const FLY_SEED: u16 = 0x51f7;

const SKY_PERIOD: u64 = 50;

const METEOR_CYCLE: u64 = 440;

const METEOR_FLIGHT: u64 = 48;

const TRAIL: u16 = 20;

const TRAIL_STEP: f32 = 0.62;

const GLOW: f32 = 0.22;

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
pub(in crate::tui::sigil_view) fn space(cols: u16, rows: u16, ticks: u64, sky: u8) -> Vec<Cell> {
    let mut cells = backdrop(cols, rows, sky);
    for spot in moving(cols, rows, ticks, sky) {
        set(&mut cells, cols, spot.x, spot.y, spot.cell);
    }
    cells
}

pub(in crate::tui::sigil_view) fn backdrop(cols: u16, rows: u16, sky: u8) -> Vec<Cell> {
    let mut cells: Vec<Cell> = field(cols, rows, look(sky).glow)
        .iter()
        .map(|haze| {
            let text = if haze.top == haze.bottom { " " } else { "▀" };
            cell(text, haze.top, haze.bottom, false)
        })
        .collect();
    for star in dust(cols, rows, sky) {
        set(&mut cells, cols, star.x, star.y, star.cell);
    }
    cells
}

pub(in crate::tui::sigil_view) fn moving(cols: u16, rows: u16, ticks: u64, sky: u8) -> Vec<Spot> {
    let mut spots = flying(cols, rows, ticks, sky);
    sparkle(&mut spots, cols, rows, ticks, sky);
    meteor(&mut spots, cols, rows, ticks, sky);
    spots
}

fn dust(cols: u16, rows: u16, sky: u8) -> Vec<Spot> {
    let look = look(sky);
    let field = field(cols, rows, look.glow);
    let mut spots = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            let haze = &field[index(cols, x, y)];
            let seed = hash(x, y);
            let chance = (DUST_BASE + DUST_BAND * haze.density.max(0.0).powf(1.4)) * look.dust;
            if (seed % 10_000) as f32 >= chance * 10_000.0 {
                continue;
            }
            let roll = (seed >> 14) % 1000;
            let warm = haze.core > 0.25 && roll.is_multiple_of(2);
            let light = starlight(haze.hue);
            let (glyph, color) = match roll {
                930.. => ("•", if warm { rgb(232, 204, 160) } else { light }),
                640.. => (
                    "∙",
                    if warm {
                        rgb(210, 180, 146)
                    } else {
                        blend(STAR_DIM, light, 0.7)
                    },
                ),
                _ => ("·", blend(STAR_DIM, light, 0.3 + 0.5 * haze.density)),
            };
            spots.push(spot(x, y, glyph, color, under(haze), false));
        }
    }
    spots
}

fn flying(cols: u16, rows: u16, ticks: u64, sky: u8) -> Vec<Spot> {
    if cols == 0 || rows == 0 {
        return Vec::new();
    }
    let look = look(sky);
    let field = field(cols, rows, look.glow);
    let wide = f32::from(cols) * 2.0;
    let tall = f32::from(rows) * 4.0;
    let count =
        ((u32::from(cols) * u32::from(rows) / STAR_AREA).max(8) as f32 * look.flying) as u32;
    let mut lit: BTreeMap<(u16, u16), (u8, usize)> = BTreeMap::new();
    for star in 0..count {
        let seed = hash(star as u16, (star >> 16) as u16 ^ FLY_SEED);
        let depth = (seed >> 28) as usize % SPEED.len();
        let travel = SPEED[depth] * ticks as f32;
        let x0 = (seed % 10_000) as f32 / 10_000.0 * wide;
        let y0 = ((seed >> 14) % 10_000) as f32 / 10_000.0 * tall;
        for tail in 0..depth.max(1) {
            let x = (x0 - travel + tail as f32).rem_euclid(wide);
            let y = (y0 + travel * SLOPE).rem_euclid(tall);
            let at = ((x / 2.0) as u16, (y / 4.0) as u16);
            let dot = DOT[(y as usize % 4) * 2 + x as usize % 2];
            let shade = depth - tail.min(depth);
            let entry = lit.entry(at).or_insert((0, shade));
            *entry = (entry.0 | dot, entry.1.max(shade));
        }
    }
    lit.into_iter()
        .map(|((x, y), (bits, shade))| {
            let bg = under(&field[index(cols, x, y)]);
            let glyph = if look.halo && shade + 1 == SPEED.len() {
                "•"
            } else {
                braille(bits)
            };
            spot(x, y, glyph, FLY[shade], bg, false)
        })
        .collect()
}

fn sparkle(spots: &mut Vec<Spot>, cols: u16, rows: u16, ticks: u64, sky: u8) {
    if !(4..20).contains(&(ticks % SKY_PERIOD)) {
        return;
    }
    let cycle = (ticks / SKY_PERIOD) as u16;
    let picked = dust(cols, rows, sky)
        .into_iter()
        .min_by_key(|star| (hash(star.x.wrapping_add(cycle), star.y), star.x, star.y));
    if let Some(star) = picked {
        let bg = star.cell.bg;
        spots.push(Spot {
            cell: Cell {
                bg,
                ..cell("✦", STAR_BRIGHT, SCRIM, true)
            },
            ..star
        });
    }
}

fn meteor(spots: &mut Vec<Spot>, cols: u16, rows: u16, ticks: u64, sky: u8) {
    let Some(flight) = flight(cols, rows, ticks) else {
        return;
    };
    let look = look(sky);
    let field = field(cols, rows, look.glow);
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
        let bg = under(&field[index(cols, x, y)]);
        spots.push(spot(x, y, braille(bits), fg, bg, false));
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
        if look.halo {
            glow(spots, &field, cols, x, y);
        }
    }
}

fn glow(spots: &mut Vec<Spot>, field: &[Field], cols: u16, x: u16, y: u16) {
    for nx in [x.checked_sub(1), x.checked_add(1).filter(|&nx| nx < cols)]
        .into_iter()
        .flatten()
    {
        let lifted = blend(under(&field[index(cols, nx, y)]), STAR_BRIGHT, GLOW);
        spots.push(spot(nx, y, " ", lifted, lifted, false));
    }
}

fn flight(cols: u16, rows: u16, ticks: u64) -> Option<Flight> {
    let phase = ticks % METEOR_CYCLE;
    if phase >= METEOR_FLIGHT || cols == 0 || rows == 0 {
        return None;
    }
    let t = phase as f32 / METEOR_FLIGHT as f32;
    let lane = LANES[(ticks / METEOR_CYCLE) as usize % LANES.len()];
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
