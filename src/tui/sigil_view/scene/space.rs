use super::*;

pub(in crate::tui::sigil_view) const SCRIM: Rgba8 = rgb(10, 8, 20);

pub(in crate::tui::sigil_view) const STAR_DIM: Rgba8 = rgb(58, 74, 110);

pub(in crate::tui::sigil_view) const STAR_MID: Rgba8 = rgb(120, 148, 200);

pub(in crate::tui::sigil_view) const STAR_BRIGHT: Rgba8 = rgb(176, 200, 240);

pub(in crate::tui::sigil_view) fn space(cols: u16, rows: u16, ticks: u64) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); usize::from(cols) * usize::from(rows)];
    let drift = (ticks / 150) as u16;
    for y in 0..rows {
        for x in 0..cols {
            let n = hash(x.wrapping_add(drift), y);
            if !n.is_multiple_of(28) {
                continue;
            }
            let phase = (ticks.wrapping_add(u64::from((n >> 7) % 192))) % 192;
            if phase <= 8 {
                continue;
            }
            let bright = n.is_multiple_of(5) && (9..=28).contains(&phase);
            set(
                &mut cells,
                cols,
                x,
                y,
                Cell {
                    text: if bright { "✦" } else { "·" }.into(),
                    fg: ink(if bright { STAR_BRIGHT } else { STAR_DIM }),
                    bg: ink(palette().base),
                    bold: false,
                    italic: false,
                    ..Cell::blank()
                },
            );
        }
    }
    let phase = ticks % 440;
    if phase < 120 {
        let progress = phase as f32 / 120.0;
        let head_x = progress * (f32::from(cols) + 16.0) - 8.0;
        let head_y = progress * f32::from(rows) * 0.4;
        for step in 0..6_u16 {
            let x = head_x - f32::from(step) * 2.0;
            let y = head_y - f32::from(step) * 0.5;
            if x < 0.0 || y < 0.0 || x >= f32::from(cols) || y >= f32::from(rows) {
                continue;
            }
            let (text, fg) = match step {
                0 => ("✦", STAR_BRIGHT),
                1 => ("·", STAR_MID),
                2.. => ("·", STAR_DIM),
            };
            set(
                &mut cells,
                cols,
                x as u16,
                y as u16,
                Cell {
                    text: text.into(),
                    fg: ink(fg),
                    bg: ink(palette().base),
                    bold: false,
                    italic: false,
                    ..Cell::blank()
                },
            );
        }
    }
    cells
}

pub(in crate::tui::sigil_view) fn hash(x: u16, y: u16) -> u32 {
    let n = u32::from(x)
        .wrapping_mul(3_747_613_937)
        .wrapping_add(u32::from(y).wrapping_mul(2_654_435_761));
    n.wrapping_mul(4_294_967_291).wrapping_add(n >> 13)
}

pub(in crate::tui::sigil_view) fn set(cells: &mut [Cell], cols: u16, x: u16, y: u16, cell: Cell) {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(slot) = cells.get_mut(index) {
        *slot = cell;
    }
}
