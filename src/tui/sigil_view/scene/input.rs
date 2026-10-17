use super::logo::{body, facts, fitting, layout};
use super::space::{GLASS, PLATE, set};
use super::*;

pub(in crate::tui::sigil_view) const ROW_MS: u64 = 70;

pub(in crate::tui::sigil_view) const ROW_WIPE_MS: u64 = 130;

pub(in crate::tui::sigil_view) const FACTS_MS: u64 = 560;

pub(in crate::tui::sigil_view) fn window_area(app: &App, cols: u16, rows: u16) -> [u16; 4] {
    let model = app.model.get();
    let facts = facts(&model, app);
    let [body_width, body_rows] = body(cols, fitting(cols, rows, &facts), &facts);
    let [_, _, x, y, width] = layout(cols, rows, body_width, body_rows);
    [x, y, width, body_rows + 2]
}

pub(in crate::tui::sigil_view) fn glass(cells: &mut [Cell], cols: u16, area: [u16; 4]) {
    let [x, y, width, height] = area;
    let rows = u16::try_from(cells.len() / usize::from(cols.max(1))).unwrap_or(u16::MAX);
    let tint = ink(Rgba8 {
        a: (GLASS * 255.0) as u8,
        ..PLATE
    });
    for row in y..y.saturating_add(height).min(rows) {
        for col in x..x.saturating_add(width).min(cols) {
            if let Some(cell) =
                cells.get_mut(usize::from(row) * usize::from(cols) + usize::from(col))
            {
                cell.bg = tint;
            }
        }
    }
}

pub(in crate::tui::sigil_view) fn overlay(
    cells: &mut [Cell],
    cols: u16,
    x: u16,
    y: u16,
    text: &str,
    color: Rgba8,
) {
    let mut col = x;
    for grapheme in text.graphemes(true) {
        if grapheme != " " {
            let bg = cells
                .get(usize::from(y) * usize::from(cols) + usize::from(col))
                .map_or_else(|| ink(palette().base), |cell| cell.bg);
            set(
                cells,
                cols,
                col,
                y,
                Cell {
                    text: grapheme.into(),
                    fg: ink(color),
                    bg,
                    bold: false,
                    italic: false,
                    ..Cell::blank()
                },
            );
        }
        col = col.saturating_add(cell_width(grapheme) as u16);
    }
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "plate_tests.rs"]
mod plate_tests;
