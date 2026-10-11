use super::logo::{cover, facts, layout};
use super::space::{PLATE, SCRIM, set};
use super::*;

pub(in crate::tui::sigil_view) const ROW_MS: u64 = 70;

pub(in crate::tui::sigil_view) const ROW_WIPE_MS: u64 = 130;

pub(in crate::tui::sigil_view) const FACTS_MS: u64 = 560;

pub(in crate::tui::sigil_view) fn prompt_area(app: &App, cols: u16, rows: u16) -> [u16; 3] {
    let model = app.model.get();
    let facts = facts(&model, app);
    let [body_width, body_rows] = cover(crate::splash::logo(), &facts);
    let [_, _, input_x, input_y, input_width] = layout(cols, rows, body_width, body_rows);
    [input_x, input_y, input_width]
}

pub(in crate::tui::sigil_view) fn dim_rect(
    cells: &mut [Cell],
    cols: u16,
    area: [u16; 4],
    reset_background: bool,
) {
    let [x, y, width, height] = area;
    let rows = u16::try_from(cells.len() / usize::from(cols.max(1))).unwrap_or(u16::MAX);
    for row in y..y.saturating_add(height).min(rows) {
        for col in x..x.saturating_add(width).min(cols) {
            let index = usize::from(row) * usize::from(cols) + usize::from(col);
            if let Some(cell) = cells.get_mut(index) {
                if reset_background {
                    cell.text = " ".into();
                    cell.fg = ink(PLATE);
                    cell.bg = ink(PLATE);
                    continue;
                }
                let reset = rgb(0, 0, 0);
                let fg = if let Ink::Exact(colour) = cell.fg {
                    colour
                } else {
                    reset
                };
                let bg = if let Ink::Exact(colour) = cell.bg {
                    colour
                } else {
                    reset
                };
                cell.fg = ink(blend(fg, SCRIM, 0.55));
                cell.bg = ink(blend(bg, SCRIM, 0.55));
            }
        }
    }
}

pub(in crate::tui::sigil_view) fn fill_bg(cells: &mut [Cell], cols: u16, area: [u16; 4]) {
    let [x, y, width, height] = area;
    let rows = u16::try_from(cells.len() / usize::from(cols.max(1))).unwrap_or(u16::MAX);
    for row in y..y.saturating_add(height).min(rows) {
        for col in x..x.saturating_add(width).min(cols) {
            let index = usize::from(row) * usize::from(cols) + usize::from(col);
            if let Some(cell) = cells.get_mut(index) {
                cell.bg = ink(PLATE);
            }
        }
    }
}

pub(in crate::tui::sigil_view) fn frame(cells: &mut [Cell], cols: u16, area: [u16; 4]) {
    let [x, y, width, height] = area;
    if width < 2 || height < 2 || cols == 0 {
        return;
    }
    let rows = u16::try_from(cells.len() / usize::from(cols)).unwrap_or(0);
    if rows == 0 {
        return;
    }
    let right = x.saturating_add(width).saturating_sub(1).min(cols - 1);
    let bottom = y.saturating_add(height).saturating_sub(1).min(rows - 1);
    if right <= x || bottom <= y {
        return;
    }
    let color = palette().gray;
    for col in x..=right {
        let (top, bottom_glyph) = if col == x {
            ("╭", "╰")
        } else if col == right {
            ("╮", "╯")
        } else {
            ("─", "─")
        };
        stroke(cells, cols, col, y, top, color);
        stroke(cells, cols, col, bottom, bottom_glyph, color);
    }
    for row in y + 1..bottom {
        stroke(cells, cols, x, row, "│", color);
        stroke(cells, cols, right, row, "│", color);
    }
}

fn stroke(cells: &mut [Cell], cols: u16, x: u16, y: u16, glyph: &str, color: Rgba8) {
    set(
        cells,
        cols,
        x,
        y,
        Cell {
            text: glyph.into(),
            fg: ink(color),
            bg: ink(PLATE),
            bold: false,
            italic: false,
            ..Cell::blank()
        },
    );
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

pub(in crate::tui::sigil_view) fn write(
    cells: &mut [Cell],
    cols: u16,
    x: u16,
    y: u16,
    text: &str,
    color: Rgba8,
) {
    write_at(
        cells,
        usize::from(cols),
        usize::from(x),
        usize::from(y),
        text,
        color,
    );
}

#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;
