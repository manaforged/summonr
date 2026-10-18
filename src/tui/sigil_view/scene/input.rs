use super::logo::{body, facts, fitting, layout};
use super::space::{GLASS, PLATE, Plate};
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

pub(in crate::tui::sigil_view) fn glass(area: [u16; 4]) -> Plate {
    let tint = ink(Rgba8 {
        a: (GLASS * 255.0) as u8,
        ..PLATE
    });
    Plate::new(
        area,
        Cell {
            bg: tint,
            ..Cell::blank()
        },
    )
}

pub(in crate::tui::sigil_view) fn overlay(
    plate: &mut Plate,
    x: u16,
    y: u16,
    text: &str,
    color: Rgba8,
) {
    let mut col = x;
    for grapheme in text.graphemes(true) {
        if grapheme != " "
            && let Some(cell) = plate.cell([col, y])
        {
            *cell = Cell {
                text: grapheme.into(),
                fg: ink(color),
                bg: cell.bg,
                bold: false,
                italic: false,
                ..Cell::blank()
            };
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

#[cfg(test)]
#[path = "idle_tests.rs"]
mod idle_tests;
