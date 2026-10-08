use super::*;

pub(in crate::tui::sigil_view) const SCRIM: Rgba8 = rgb(10, 8, 20);

pub(in crate::tui::sigil_view) const PLATE: Rgba8 = rgb(42, 44, 58);

pub(in crate::tui::sigil_view) const STAR_DIM: Rgba8 = rgb(58, 74, 110);

pub(in crate::tui::sigil_view) const STAR_MID: Rgba8 = rgb(120, 148, 200);

pub(in crate::tui::sigil_view) const STAR_BRIGHT: Rgba8 = rgb(176, 200, 240);

const SKY_STEP: u64 = 8;
const SKY_PERIOD: u64 = 480;
const LANES: [[f32; 4]; 6] = [
    [-0.08, 0.02, 1.08, 0.42],
    [-0.08, 0.48, 1.05, 0.78],
    [-0.08, 0.82, 0.72, 1.08],
    [1.08, 0.08, -0.08, 0.62],
    [0.15, -0.12, 0.78, 1.08],
    [0.88, 1.12, 0.22, -0.08],
];

pub(in crate::tui::sigil_view) fn space(cols: u16, rows: u16, ticks: u64) -> Vec<Cell> {
    let mut cells = vec![night(); usize::from(cols) * usize::from(rows)];
    stars(&mut cells, cols, rows, ticks);
    meteor(&mut cells, cols, rows, ticks);
    cells
}

fn night() -> Cell {
    Cell {
        text: " ".into(),
        fg: ink(SCRIM),
        bg: ink(SCRIM),
        bold: false,
        italic: false,
        ..Cell::blank()
    }
}

fn stars(cells: &mut [Cell], cols: u16, rows: u16, ticks: u64) {
    let sky = ticks / SKY_STEP;
    let drift = (sky / 150) as u16;
    for y in 0..rows {
        for x in 0..cols {
            let n = hash(x.wrapping_add(drift), y);
            if !n.is_multiple_of(28) {
                continue;
            }
            let phase = (sky.wrapping_add(u64::from((n >> 7) % SKY_PERIOD as u32))) % SKY_PERIOD;
            if phase < 16 {
                continue;
            }
            let bright = n.is_multiple_of(5) && (16..40).contains(&phase);
            let (text, fg) = if bright {
                ("✦", STAR_BRIGHT)
            } else {
                ("·", STAR_DIM)
            };
            set(cells, cols, x, y, star(text, fg));
        }
    }
}

fn meteor(cells: &mut [Cell], cols: u16, rows: u16, ticks: u64) {
    let phase = ticks % 440;
    if phase >= 120 {
        return;
    }
    let progress = phase as f32 / 120.0;
    let lane = LANES[(ticks / 440) as usize % LANES.len()];
    let head_x = lerp(lane[0], lane[2], progress) * f32::from(cols);
    let head_y = lerp(lane[1], lane[3], progress) * f32::from(rows);
    let dx = (lane[2] - lane[0]) * f32::from(cols);
    let dy = (lane[3] - lane[1]) * f32::from(rows);
    let span = (dx * dx + dy * dy).sqrt().max(1.0);
    for step in 0..6_u16 {
        let along = f32::from(step) * 2.0;
        let x = head_x - dx / span * along;
        let y = head_y - dy / span * along;
        if x < 0.0 || y < 0.0 || x >= f32::from(cols) || y >= f32::from(rows) {
            continue;
        }
        let (text, fg) = match step {
            0 => ("✦", STAR_BRIGHT),
            1 => ("·", STAR_MID),
            2.. => ("·", STAR_DIM),
        };
        set(cells, cols, x as u16, y as u16, star(text, fg));
    }
}

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

fn star(text: &'static str, fg: Rgba8) -> Cell {
    Cell {
        text: text.into(),
        fg: ink(fg),
        bg: ink(SCRIM),
        bold: false,
        italic: false,
        ..Cell::blank()
    }
}

pub(in crate::tui::sigil_view) fn hash(x: u16, y: u16) -> u32 {
    let n = u32::from(x)
        .wrapping_mul(3_747_613_937)
        .wrapping_add(u32::from(y).wrapping_mul(2_654_435_761));
    n.wrapping_mul(4_294_967_291).wrapping_add(n >> 13)
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
