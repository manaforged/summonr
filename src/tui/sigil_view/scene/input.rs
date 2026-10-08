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

pub(in crate::tui::sigil_view) fn clear(
    cells: &mut [Cell],
    cols: u16,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            set(cells, cols, col, row, Cell::blank());
        }
    }
}

pub(in crate::tui::sigil_view) fn prompt(
    cells: &mut [Cell],
    cols: u16,
    area: [u16; 3],
    model: &Model,
    app: &App,
    visible: bool,
    ticks: u64,
) {
    let [x, y, width] = area;
    let (mode, glyph, mode_color) = if model.snapshot.plan {
        ("plan", '◇', palette().cyan)
    } else if model.snapshot.work {
        ("work", '▣', palette().yellow)
    } else if model.snapshot.goal {
        ("goal", '◎', palette().red)
    } else {
        (model.snapshot.mode_name(), '●', palette().green)
    };
    let busy = model.busy_since.is_some();
    let accent = palette().accent;
    let border = if busy { accent } else { palette().gray };
    let right_width = usize::from(width).saturating_sub(6 + mode.len());
    let right = crate::text::ellipsize(&model.snapshot.model, right_width);
    let fill = usize::from(width).saturating_sub(6 + mode.len() + cell_width(right.as_str()));
    write(
        cells,
        cols,
        x,
        y + 1,
        &format!("╭ {glyph} {mode} {}", "─".repeat(fill)),
        border,
    );
    write(
        cells,
        cols,
        x + 1,
        y + 1,
        &format!(" {glyph} {mode} "),
        mode_color,
    );
    write(
        cells,
        cols,
        x + width.saturating_sub(cell_width(right.as_str()) as u16 + 2),
        y + 1,
        &right,
        palette().dim,
    );
    write(cells, cols, x + width.saturating_sub(1), y + 1, "╮", border);
    write(cells, cols, x, y + 2, "│", border);
    let interior = usize::from(width.saturating_sub(2));
    for col in 1..interior + 1 {
        set(
            cells,
            cols,
            x + col as u16,
            y + 2,
            Cell {
                text: " ".into(),
                fg: ink(palette().text),
                bg: ink(SCRIM),
                bold: false,
                italic: false,
                ..Cell::blank()
            },
        );
    }
    if visible && app.view_edit.peek() {
        let path = app
            .model
            .get()
            .view
            .as_ref()
            .map(|view| view.path.clone())
            .unwrap_or_default();
        write(
            cells,
            cols,
            x + 3,
            y + 2,
            &format!(
                "editing {path} · {}",
                crate::tui::sigil_view::ui::Hints::new()
                    .key(crate::tui::sigil_view::ui::Key::Fixed("ctrl+s"), "save")
                    .key(crate::tui::sigil_view::ui::Key::Esc, "leave")
                    .text(&model.prefs.hotkeys)
            ),
            palette().dim,
        );
    } else if visible {
        write(cells, cols, x + 3, y + 2, "❯ ", mode_color);
        let caret = if (ticks / 6).is_multiple_of(2) {
            '│'
        } else {
            ' '
        };
        let input = super::chat::draft_view(
            &app.draft.get(),
            app.caret.get(),
            usize::from(width.saturating_sub(8)),
            caret,
        );
        write(cells, cols, x + 5, y + 2, &input, palette().text);
    }
    write(cells, cols, x + width.saturating_sub(1), y + 2, "│", border);
    let select = app
        .select
        .peek()
        .then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
    if let Some(status) = super::chat::busy_status(model, app.ticks.peek()).filter(|_| y > 1) {
        let shown = crate::text::ellipsize(&status, usize::from(width));
        let (spinner, rest) = shown.split_at(shown.find(' ').unwrap_or(shown.len()));
        write(cells, cols, x, y.saturating_sub(1), spinner, accent);
        let after = x.saturating_add(cell_width(spinner) as u16);
        write(cells, cols, after, y.saturating_sub(1), rest, palette().dim);
    }
    let hint = super::chat::composer_hint(
        model.busy_since.map(|(activity, _)| activity),
        usize::from(width),
        select.as_deref(),
    );
    let dash = usize::from(width).saturating_sub(cell_width(hint.as_str()) + 2);
    write(
        cells,
        cols,
        x,
        y + 3,
        &format!("╰{}", "─".repeat(dash)),
        border,
    );
    write(
        cells,
        cols,
        x + 1 + dash as u16,
        y + 3,
        &hint,
        palette().dim,
    );
    write(cells, cols, x + width.saturating_sub(1), y + 3, "╯", border);
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
