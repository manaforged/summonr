use super::*;

pub(in crate::tui::sigil_view) const SCRIM: Rgba8 = rgb(10, 8, 20);

pub(in crate::tui::sigil_view) const PLATE: Rgba8 = rgb(42, 44, 58);

pub(in crate::tui::sigil_view) const STAR_DIM: Rgba8 = rgb(58, 74, 110);

pub(in crate::tui::sigil_view) const STAR_MID: Rgba8 = rgb(120, 148, 200);

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

struct Look {
    far_every: u32,
    near_every: u32,
    band: f32,
    halo: bool,
}

fn look(sky: u8) -> Look {
    let dust = sky % 3 == DUST;
    Look {
        far_every: if dust { 14 } else { FAR_EVERY },
        near_every: if dust { 64 } else { NEAR_EVERY },
        band: if dust { 1.0 } else { 0.7 },
        halo: sky % 3 == HALO,
    }
}

const BAND: Rgba8 = rgb(36, 18, 62);

const AIR: Rgba8 = rgb(16, 28, 52);

const VOID: Rgba8 = rgb(4, 3, 10);

const WARM: Rgba8 = rgb(214, 176, 122);

const FLARE: Rgba8 = rgb(244, 246, 255);

const FAR_EVERY: u32 = 30;

const FAR_DRIFT: u64 = 62;

const NEAR_EVERY: u32 = 96;

const NEAR_DRIFT: u64 = 28;

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

pub(in crate::tui::sigil_view) fn space(cols: u16, rows: u16, ticks: u64, sky: u8) -> Vec<Cell> {
    let look = look(sky);
    let mut cells = backdrop(cols, rows, look.band);
    stars(&mut cells, cols, rows, ticks, &look);
    meteor(&mut cells, cols, rows, ticks, look.halo);
    cells
}

fn backdrop(cols: u16, rows: u16, band: f32) -> Vec<Cell> {
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for y in 0..rows {
        let tint = row_tint(y, rows, band);
        for x in 0..cols {
            cells.push(night(shade(tint, x, cols)));
        }
    }
    cells
}

fn row_tint(y: u16, rows: u16, mix: f32) -> Rgba8 {
    if rows <= 1 {
        return SCRIM;
    }
    let t = f32::from(y) / f32::from(rows - 1);
    let band = (-((t - 0.36) / 0.18).powi(2)).exp();
    let air = (1.0 - t * 3.2).max(0.0);
    blend(blend(SCRIM, BAND, band * mix), AIR, air * 0.35)
}

fn shade(tint: Rgba8, x: u16, cols: u16) -> Rgba8 {
    let margin = 6u16.min(cols / 5);
    if margin == 0 {
        return tint;
    }
    let x = i32::from(x);
    let cols = i32::from(cols);
    let margin = i32::from(margin);
    let edge = if x < margin {
        (margin - x) as f32 / margin as f32
    } else if cols - x <= margin {
        (margin - (cols - x) + 1) as f32 / margin as f32
    } else {
        0.0
    };
    blend(tint, VOID, edge * 0.55)
}

fn stars(cells: &mut [Cell], cols: u16, rows: u16, ticks: u64, look: &Look) {
    layer(cells, cols, rows, ticks, look, false);
    layer(cells, cols, rows, ticks, look, true);
    sparkle(cells, cols, rows, ticks);
}

fn layer(cells: &mut [Cell], cols: u16, rows: u16, ticks: u64, look: &Look, near: bool) {
    let drift = (ticks / if near { NEAR_DRIFT } else { FAR_DRIFT }) as u16;
    let every = if near {
        look.near_every
    } else {
        look.far_every
    };
    let lift = if near { 0.42 } else { 0.0 };
    for y in 0..rows {
        for x in 0..cols {
            let n = hash(x.wrapping_add(drift), y);
            if !n.is_multiple_of(every) {
                continue;
            }
            let color = blend(tone(n), STAR_BRIGHT, lift);
            put(cells, cols, x, y, "·", color, near);
            if near && look.halo {
                glow(cells, cols, rows, x, y, color);
            }
        }
    }
}

fn tone(n: u32) -> Rgba8 {
    match (n >> 20) % 8 {
        0 => WARM,
        1 => rgb(186, 206, 230),
        2 | 3 => STAR_MID,
        _ => STAR_DIM,
    }
}

fn sparkle(cells: &mut [Cell], cols: u16, rows: u16, ticks: u64) {
    if !(4..20).contains(&(ticks % SKY_PERIOD)) {
        return;
    }
    let drift = (ticks / FAR_DRIFT) as u16;
    let cycle = (ticks / SKY_PERIOD) as u16;
    let mut best: Option<(u32, u16, u16)> = None;
    for y in 0..rows {
        for x in 0..cols {
            let n = hash(x.wrapping_add(drift), y);
            if !n.is_multiple_of(FAR_EVERY) {
                continue;
            }
            let candidate = (hash(x.wrapping_add(cycle), y), x, y);
            best = Some(best.map_or(candidate, |current| current.min(candidate)));
        }
    }
    if let Some((_, x, y)) = best {
        put(cells, cols, x, y, "✦", STAR_BRIGHT, true);
    }
}

fn meteor(cells: &mut [Cell], cols: u16, rows: u16, ticks: u64, halo: bool) {
    let Some(flight) = flight(cols, rows, ticks) else {
        return;
    };
    for step in (1..TRAIL).rev() {
        let along = f32::from(step) * TRAIL_STEP;
        let fade = 1.0 - f32::from(step) / f32::from(TRAIL);
        let fg = if step < 4 {
            blend(WARM, FLARE, fade)
        } else {
            blend(STAR_DIM, WARM, fade)
        };
        plot_dot(
            cells,
            cols,
            rows,
            flight.x - flight.ux * along,
            flight.y - flight.uy * along,
            fg,
        );
    }
    if (0.0..f32::from(cols)).contains(&flight.x) && (0.0..f32::from(rows)).contains(&flight.y) {
        put(
            cells,
            cols,
            flight.x as u16,
            flight.y as u16,
            "✦",
            FLARE,
            true,
        );
        if halo {
            glow(cells, cols, rows, flight.x as u16, flight.y as u16, FLARE);
        }
    }
}

fn glow(cells: &mut [Cell], cols: u16, rows: u16, x: u16, y: u16, color: Rgba8) {
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let nx = i32::from(x) + dx;
        let ny = i32::from(y) + dy;
        if nx < 0 || ny < 0 || nx >= i32::from(cols) || ny >= i32::from(rows) {
            continue;
        }
        let index = ny as usize * usize::from(cols) + nx as usize;
        let Some(cell) = cells.get_mut(index) else {
            continue;
        };
        if cell.text != " " {
            continue;
        }
        let bg = match cell.bg {
            Ink::Exact(color) => color,
            _ => SCRIM,
        };
        let lifted = blend(bg, color, 0.45);
        cell.bg = ink(lifted);
        cell.fg = ink(lifted);
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

fn plot_dot(cells: &mut [Cell], cols: u16, rows: u16, x: f32, y: f32, fg: Rgba8) {
    if x < 0.0 || y < 0.0 || x >= f32::from(cols) || y >= f32::from(rows) {
        return;
    }
    let ix = x as u16;
    let iy = y as u16;
    let subx = usize::from(x.fract() >= 0.5);
    let suby = ((y.fract() * 4.0) as usize).min(3);
    let index = usize::from(iy) * usize::from(cols) + usize::from(ix);
    let Some(cell) = cells.get_mut(index) else {
        return;
    };
    if cell.text == "✦" {
        return;
    }
    let bits = braille_bits(&cell.text).unwrap_or(0) | DOT[suby * 2 + subx];
    cell.text = braille(bits).to_string().into();
    cell.fg = ink(fg);
    cell.bold = false;
}

fn braille(bits: u8) -> char {
    char::from_u32(0x2800 | u32::from(bits)).unwrap_or('·')
}

fn braille_bits(text: &str) -> Option<u8> {
    let code = u32::from(text.chars().next()?);
    (0x2800..=0x28FF)
        .contains(&code)
        .then_some((code & 0xFF) as u8)
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn night(color: Rgba8) -> Cell {
    Cell {
        text: " ".into(),
        fg: ink(color),
        bg: ink(color),
        bold: false,
        italic: false,
        ..Cell::blank()
    }
}

fn put(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &'static str, fg: Rgba8, bold: bool) {
    if x >= cols {
        return;
    }
    let bg = bg_of(cells, cols, x, y);
    set(
        cells,
        cols,
        x,
        y,
        Cell {
            text: text.into(),
            fg: ink(fg),
            bg: ink(bg),
            bold,
            italic: false,
            ..Cell::blank()
        },
    );
}

fn bg_of(cells: &[Cell], cols: u16, x: u16, y: u16) -> Rgba8 {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(Ink::Exact(color)) = cells.get(index).map(|cell| cell.bg) {
        return color;
    }
    SCRIM
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
