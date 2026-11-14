use super::logo::{body, facts, fitting, layout};
#[cfg(test)]
use super::space::PLATE;
use super::space::Plate;
use super::*;

pub(crate) fn window_area(app: &App, cols: u16, rows: u16) -> [u16; 4] {
    let model = app.model.get();
    let facts = facts(&model, app);
    if super::lockup::drawn(model.prefs.splash) {
        return super::lockup::window_area(cols, rows, &facts);
    }
    let [body_width, body_rows] = body(cols, fitting(cols, rows, &facts), &facts);
    let [_, _, x, y, width] = layout(cols, rows, body_width, body_rows);
    [x, y, width, body_rows + 2]
}

pub(crate) fn glass(area: [u16; 4]) -> Plate {
    let colours = stage();
    let tint = ink(Color {
        a: (colours.glass * 255.0) as u8,
        ..colours.plate
    });
    let mut blank = Cell::blank();
    blank.bg = tint;
    Plate::new(area, blank)
}

pub(crate) fn overlay(plate: &mut Plate, x: u16, y: u16, text: &str, color: Color) {
    let mut col = x;
    for grapheme in text.graphemes(true) {
        if grapheme != " "
            && let Some(cell) = plate.cell([col, y])
        {
            let bg = cell.bg;
            *cell = Cell::blank();
            cell.set_text(grapheme);
            cell.fg = ink(color);
            cell.bg = bg;
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
