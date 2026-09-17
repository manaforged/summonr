use std::ops::Range;

use super::app::App;
use super::canvas::{Cell, palette};
use super::chat::{accent, draft_view, ellipsize, write_at};
use super::input::Input;
use sigil::prelude::{NodeBuilder, Signal, signal, text_edit};

#[derive(Clone, Copy)]
pub(super) struct View {
    pub(super) open: Signal<bool>,
    pub(super) query: Signal<String>,
    pub(super) caret: Signal<usize>,
    pub(super) offset: Signal<usize>,
}

impl View {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            query: signal(String::new()),
            caret: signal(0),
            offset: signal(0),
        }
    }

    pub(super) fn show(self) {
        self.query.set(String::new());
        self.caret.set(0);
        self.offset.set(0);
        self.open.set(true);
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let help = app.help;
    let input = text_edit(help.query)
        .caret_position(help.caret)
        .change(move |_| help.offset.set(0))
        .class("w-full h-full");
    super::settings::layer(app, grid, input)
}

pub(super) fn shift(app: &App, delta: isize) {
    let (width, height) = app.viewport.peek();
    if height < 6 {
        return;
    }
    let (_, rows) = content(app, usize::from(width));
    let last = rows
        .len()
        .saturating_sub(usize::from(height).saturating_sub(5));
    let next = start(&rows, app.help.offset.peek(), last)
        .saturating_add_signed(delta)
        .min(last);
    app.help
        .offset
        .set(rows.get(next).map_or(0, |row| row.start));
}

fn start(rows: &[Range<usize>], offset: usize, last: usize) -> usize {
    rows.partition_point(|row| row.start <= offset)
        .saturating_sub(1)
        .min(last)
}

fn content(app: &App, width: usize) -> (String, Vec<Range<usize>>) {
    let mut text = super::super::help_text(&app.model.borrow().prefs.hotkeys);
    let query = app.help.query.get().to_lowercase();
    let terms: Vec<_> = query.split_whitespace().collect();
    if !terms.is_empty() {
        text = text
            .lines()
            .filter(|line| {
                let line = line.to_lowercase();
                terms.iter().all(|term| line.contains(term))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    let left = usize::from(width > 2);
    let rows = if text.is_empty() {
        Vec::new()
    } else {
        Input::new(&text, width.saturating_sub(left * 2)).rows
    };
    (text, rows)
}

fn grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    app.hits.clear();
    let left = usize::from(width > 2);
    let columns = width.saturating_sub(left * 2);
    let color = accent(&app.model.borrow().prefs.theme);
    if height < 6 {
        write_at(
            &mut cells,
            width,
            left,
            0,
            &ellipsize("Help · esc close", columns),
            color,
        );
        if height > 1 {
            write_at(
                &mut cells,
                width,
                left,
                1,
                &ellipsize("Enlarge terminal to read help.", columns),
                palette().dim,
            );
        }
        return cells;
    }
    write_at(
        &mut cells,
        width,
        left,
        0,
        &ellipsize("Help · keys and commands", columns),
        color,
    );
    let query = draft_view(
        &app.help.query.get(),
        app.help.caret.get(),
        columns.saturating_sub(7),
        '▌',
    );
    write_at(
        &mut cells,
        width,
        left,
        2,
        &ellipsize(&format!("find   {query}"), columns),
        palette().text,
    );
    let (text, rows) = content(app, width);
    let count = height.saturating_sub(5);
    let start = start(
        &rows,
        app.help.offset.get(),
        rows.len().saturating_sub(count),
    );
    if rows.is_empty() {
        write_at(
            &mut cells,
            width,
            left,
            4,
            &ellipsize("No matches. Edit the filter.", columns),
            palette().dim,
        );
    }
    for (offset, range) in rows.iter().skip(start).take(count).enumerate() {
        write_at(
            &mut cells,
            width,
            left,
            4 + offset,
            &text[range.clone()],
            palette().text,
        );
    }
    let position = if rows.is_empty() {
        "0 results".to_string()
    } else {
        format!(
            "{}–{}/{}",
            start + 1,
            (start + count).min(rows.len()),
            rows.len()
        )
    };
    let hint = format!("{position} · ↑↓ scroll · pgup/pgdn · esc close");
    let hint = if sigil_tui::cell_width(&hint) <= columns {
        hint
    } else {
        format!("{position} · ↑↓ · esc close")
    };
    write_at(
        &mut cells,
        width,
        left,
        height.saturating_sub(1),
        &ellipsize(&hint, columns),
        palette().dim,
    );
    cells
}
