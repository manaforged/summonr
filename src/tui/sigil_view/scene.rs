use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::app::App;
use super::canvas::{Cell, blend, cell as metrics, palette, rgb};
use super::chat::write_at;
use super::model::Model;
use crate::tui::short_session;
use sigil::prelude::{
    ColorAtom, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Position, Rgba8, Style,
    container, label,
};

const SCRIM: Rgba8 = rgb(10, 8, 20);
const STAR_DIM: Rgba8 = rgb(58, 74, 110);
const STAR_MID: Rgba8 = rgb(120, 148, 200);
const STAR_BRIGHT: Rgba8 = rgb(176, 200, 240);
const ROW_MS: u64 = 70;
const ROW_WIPE_MS: u64 = 130;
const FACTS_MS: u64 = 560;

pub(super) fn layer(app: App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset(0.0)
                .fill_width()
                .fill_height()
                .gap(0.0),
        )
        .children_reactive(move || {
            super::view::watch(&app);
            let (cols, rows) = app.viewport.get();
            drop(app.theme_name.get());
            let ticks = if app.model.borrow().prefs.animations {
                app.ticks.get()
            } else {
                0
            };
            let mut cells = space(cols, rows, ticks);
            compose(&mut cells, cols, rows, &app, ticks);
            cells
                .chunks(usize::from(cols).max(1))
                .map(scene_row)
                .collect::<Vec<_>>()
        })
}

fn compose(cells: &mut [Cell], cols: u16, rows: u16, app: &App, ticks: u64) {
    let width = usize::from(cols);
    if width == 0 || rows == 0 {
        return;
    }
    let model = app.model.borrow();
    let facts = facts(&model, app);
    let logo = crate::splash::logo();
    let [body_width, body_rows] = cover(logo, &facts);
    let [body_x, body_y, input_x, input_y, input_width] = layout(cols, rows, body_width, body_rows);
    let scrim_x = body_x.saturating_sub(2);
    let scrim_y = body_y.saturating_sub(1);
    dim_rect(
        cells,
        cols,
        [
            scrim_x,
            scrim_y,
            (body_width + 4).min(cols.saturating_sub(scrim_x)),
            (body_rows + 2).min(rows.saturating_sub(scrim_y)),
        ],
        true,
    );
    let accent = palette().accent;
    let revealed =
        app.splash_revealed.peek() || !model.prefs.animations || !app.draft.peek().is_empty();
    let elapsed = if revealed {
        u64::MAX / 2
    } else {
        ticks.saturating_mul(40)
    };
    let facts_visible = elapsed >= FACTS_MS;
    let logo_width = crate::splash::logo_width() as u16;
    let logo_x = body_x + body_width.saturating_sub(logo_width) / 2;
    let facts_x = body_x;
    for (row, text) in logo.iter().enumerate() {
        let width = text.chars().count();
        let shown = reveal_chars(row, width, elapsed);
        for (col, glyph) in text.chars().take(shown).enumerate() {
            if glyph != ' ' {
                overlay(
                    cells,
                    cols,
                    logo_x + col as u16,
                    body_y + row as u16,
                    &glyph.to_string(),
                    bloom(row, col, width, elapsed, accent),
                );
            }
        }
    }
    let facts_y = body_y + logo.len() as u16 + 1;
    for (row, (label, value)) in facts.iter().enumerate().filter(|_| facts_visible) {
        write(
            cells,
            cols,
            facts_x,
            facts_y + row as u16,
            &format!("{label:<8} "),
            palette().dim,
        );
        write(
            cells,
            cols,
            facts_x + 9,
            facts_y + row as u16,
            value,
            palette().gray,
        );
    }
    clear(
        cells,
        cols,
        input_x,
        input_y.saturating_sub(1),
        input_width,
        4,
    );
    prompt(
        cells,
        cols,
        [input_x, input_y, input_width],
        &model,
        app,
        facts_visible,
        ticks,
    );
    drop(model);
    if let Some(menu) = super::suggest::menu(app, cols, rows) {
        let left = menu.left as u16;
        let top = menu.top as u16;
        let width = menu
            .lines
            .first()
            .map(|line| cell_width(line.as_str()) as u16)
            .unwrap_or(0);
        clear(cells, cols, left, top, width, menu.lines.len() as u16);
        for (offset, line) in menu.lines.iter().enumerate() {
            write(
                cells,
                cols,
                left,
                top.saturating_add(offset as u16),
                line,
                palette().text,
            );
        }
    }
}

pub(super) fn prompt_area(app: &App, cols: u16, rows: u16) -> [u16; 3] {
    let model = app.model.borrow();
    let facts = facts(&model, app);
    let [body_width, body_rows] = cover(crate::splash::logo(), &facts);
    let [_, _, input_x, input_y, input_width] = layout(cols, rows, body_width, body_rows);
    [input_x, input_y, input_width]
}

fn facts(model: &Model, app: &App) -> Vec<(&'static str, String)> {
    let shown_model = model.snapshot.relay.as_deref().map_or_else(
        || model.snapshot.model.clone(),
        |relay| format!("{} · {relay}", model.snapshot.model),
    );
    let mut facts = vec![
        ("summonr", app.info.version.clone()),
        ("model", shown_model),
        ("session", short_session(&model.session)),
        ("workdir", app.info.workspace.clone()),
        ("setup", "/auth · /model · /help".to_string()),
    ];
    if model.snapshot.plan {
        facts.push(("mode", "plan · read-only".to_string()));
    }
    if model.snapshot.work {
        facts.push(("mode", "work · todos and verify".to_string()));
    }
    if model.snapshot.strict {
        facts.push(("policy", "strict".to_string()));
    }
    facts
}

fn cover(logo: &[&str; 6], facts: &[(&str, String)]) -> [u16; 2] {
    let logo_width = crate::splash::logo_width() as u16;
    let facts_width = facts
        .iter()
        .map(|(label, value)| format!("{label:<8} {value}").chars().count() as u16)
        .max()
        .unwrap_or(20);
    [
        logo_width.max(facts_width),
        logo.len() as u16 + facts.len() as u16 + 2,
    ]
}

fn layout(cols: u16, rows: u16, body_width: u16, body_rows: u16) -> [u16; 5] {
    let gutter = if cols > 40 {
        16
    } else if cols > 28 {
        8
    } else {
        2
    };
    let input_width = cols.saturating_sub(gutter).min(72);
    let total = body_rows + 5;
    let body_x = cols.saturating_sub(body_width) / 2;
    let input_x = cols.saturating_sub(input_width) / 2;
    let (body_y, input_y) = if rows >= total + 2 {
        let top = rows.saturating_sub(total) / 2;
        (top, top + body_rows + 2)
    } else {
        (rows.saturating_sub(body_rows) / 2, rows.saturating_sub(3))
    };
    [body_x, body_y, input_x, input_y, input_width]
}

fn reveal_chars(row: usize, len: usize, elapsed: u64) -> usize {
    let start = row as u64 * ROW_MS;
    if elapsed <= start {
        0
    } else if elapsed >= start + ROW_WIPE_MS {
        len
    } else {
        len * (elapsed - start) as usize / ROW_WIPE_MS as usize
    }
}

fn bloom(row: usize, col: usize, width: usize, elapsed: u64, accent: Rgba8) -> Rgba8 {
    let appeared = row as u64 * ROW_MS + col as u64 * ROW_WIPE_MS / width.max(1) as u64;
    let age = elapsed.saturating_sub(appeared);
    if age >= 450 {
        accent
    } else if age >= 150 {
        rgb(120, 148, 200)
    } else {
        STAR_BRIGHT
    }
}

fn dim_rect(cells: &mut [Cell], cols: u16, area: [u16; 4], reset_background: bool) {
    let [x, y, width, height] = area;
    let rows = u16::try_from(cells.len() / usize::from(cols.max(1))).unwrap_or(u16::MAX);
    for row in y..y.saturating_add(height).min(rows) {
        for col in x..x.saturating_add(width).min(cols) {
            let index = usize::from(row) * usize::from(cols) + usize::from(col);
            if let Some(cell) = cells.get_mut(index) {
                let reset = rgb(0, 0, 0);
                let fg = if reset_background && cell.text == " " {
                    reset
                } else {
                    cell.fg
                };
                let bg = if reset_background { reset } else { cell.bg };
                cell.fg = blend(fg, SCRIM, 0.55);
                cell.bg = blend(bg, SCRIM, 0.55);
            }
        }
    }
}

fn overlay(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &str, color: Rgba8) {
    let mut col = x;
    for grapheme in text.graphemes(true) {
        if grapheme != " " {
            let bg = cells
                .get(usize::from(y) * usize::from(cols) + usize::from(col))
                .map_or(palette().base, |cell| cell.bg);
            set(
                cells,
                cols,
                col,
                y,
                Cell {
                    text: grapheme.into(),
                    fg: color,
                    bg,
                    bold: false,
                    italic: false,
                },
            );
        }
        col = col.saturating_add(cell_width(grapheme) as u16);
    }
}

fn clear(cells: &mut [Cell], cols: u16, x: u16, y: u16, width: u16, height: u16) {
    for row in y..y.saturating_add(height) {
        for col in x..x.saturating_add(width) {
            set(cells, cols, col, row, Cell::blank());
        }
    }
}

fn prompt(
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
        ("auto", '●', palette().green)
    };
    let busy = model.busy_since.is_some();
    let accent = palette().accent;
    let border = if busy { accent } else { palette().gray };
    let right_width = usize::from(width).saturating_sub(6 + mode.len());
    let right = super::chat::ellipsize(&model.snapshot.model, right_width);
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
    if visible && app.view_edit.peek() {
        let path = app
            .model
            .borrow()
            .view
            .as_ref()
            .map(|view| view.path.clone())
            .unwrap_or_default();
        write(
            cells,
            cols,
            x + 3,
            y + 2,
            &format!("editing {path} · ctrl+s save · esc leave"),
            palette().dim,
        );
    } else if visible {
        write(cells, cols, x + 3, y + 2, "❯ ", mode_color);
        let caret = if (ticks / 6).is_multiple_of(2) {
            '▌'
        } else {
            ' '
        };
        let input = super::chat::draft_view(
            &app.draft.peek(),
            app.caret.peek(),
            usize::from(width.saturating_sub(8)),
            caret,
        );
        write(cells, cols, x + 5, y + 2, &input, palette().text);
    }
    write(cells, cols, x + width.saturating_sub(1), y + 2, "│", border);
    let options = model.prefs.hotkeys.label(crate::prefs::Hotkey::Options);
    let select = app
        .select
        .peek()
        .then(|| model.prefs.hotkeys.label(crate::prefs::Hotkey::Select));
    if let Some(status) = super::chat::busy_status(model).filter(|_| y > 1) {
        let shown = super::chat::ellipsize(&status, usize::from(width));
        let (spinner, rest) = shown.split_at(shown.find(' ').unwrap_or(shown.len()));
        write(cells, cols, x, y.saturating_sub(1), spinner, accent);
        let after = x.saturating_add(cell_width(spinner) as u16);
        write(cells, cols, after, y.saturating_sub(1), rest, palette().dim);
    }
    let hint = super::chat::composer_hint(
        model.busy_since.map(|(activity, _)| activity),
        usize::from(width),
        &options,
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

fn write(cells: &mut [Cell], cols: u16, x: u16, y: u16, text: &str, color: Rgba8) {
    write_at(
        cells,
        usize::from(cols),
        usize::from(x),
        usize::from(y),
        text,
        color,
    );
}

fn scene_row(cells: &[Cell]) -> NodeBuilder {
    let mut row = container().layout(
        LayoutStyle::flex_row()
            .height(metrics().h)
            .fill_width()
            .gap(0.0),
    );
    let mut start = 0;
    while start < cells.len() {
        let cell = &cells[start];
        let mut end = start + 1;
        while end < cells.len()
            && cells[end].fg == cell.fg
            && cells[end].bg == cell.bg
            && cells[end].bold == cell.bold
            && cells[end].italic == cell.italic
        {
            end += 1;
        }
        let text: String = cells[start..end]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect();
        let surface = Style::new().bg(cell.bg);
        let ink = Style::new().text_color(ColorAtom::Rgba8(cell.fg));
        let class = if cell.bold {
            "font-mono font-bold"
        } else {
            "font-mono"
        };
        row = row.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .width((end - start) as f32 * metrics().w)
                        .height(metrics().h),
                )
                .style(surface)
                .child(label(text).class(class).style(ink)),
        );
        start = end;
    }
    row
}

fn space(cols: u16, rows: u16, ticks: u64) -> Vec<Cell> {
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
                    fg: if bright { STAR_BRIGHT } else { STAR_DIM },
                    bg: palette().base,
                    bold: false,
                    italic: false,
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
                _ => ("·", STAR_DIM),
            };
            set(
                &mut cells,
                cols,
                x as u16,
                y as u16,
                Cell {
                    text: text.into(),
                    fg,
                    bg: palette().base,
                    bold: false,
                    italic: false,
                },
            );
        }
    }
    cells
}

fn hash(x: u16, y: u16) -> u32 {
    let n = u32::from(x)
        .wrapping_mul(3_747_613_937)
        .wrapping_add(u32::from(y).wrapping_mul(2_654_435_761));
    n.wrapping_mul(4_294_967_291).wrapping_add(n >> 13)
}

fn set(cells: &mut [Cell], cols: u16, x: u16, y: u16, cell: Cell) {
    let index = usize::from(y) * usize::from(cols) + usize::from(x);
    if let Some(slot) = cells.get_mut(index) {
        *slot = cell;
    }
}
