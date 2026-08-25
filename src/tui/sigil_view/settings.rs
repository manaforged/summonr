use sigil::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::canvas::{Cell, DIM, GRAY, TEXT};
use super::chat::{BLACK, RED, YELLOW, accent, draft_view, ellipsize, row, write_at};

pub(super) fn settings_layer(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let cells = settings_grid(&app, usize::from(cols), usize::from(rows));
    let mut layer = container().layout(
        LayoutStyle::flex_column()
            .position(Position::Absolute)
            .inset(0.0)
            .fill_width()
            .fill_height()
            .gap(0.0),
    );
    for line in cells.chunks(usize::from(cols).max(1)) {
        layer = layer.child(row(line));
    }
    container()
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(text_field(signal(String::new())).class("w-full h-full"))
        .child(layer)
}

pub(super) fn model_picker_layer(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let cells = model_picker_grid(&app, usize::from(cols), usize::from(rows));
    let mut layer = container().layout(
        LayoutStyle::flex_column()
            .position(Position::Absolute)
            .inset(0.0)
            .fill_width()
            .fill_height()
            .gap(0.0),
    );
    for line in cells.chunks(usize::from(cols).max(1)) {
        layer = layer.child(row(line));
    }
    container()
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(
            text_edit(app.model_draft)
                .caret_position(app.model_caret)
                .class("w-full h-full"),
        )
        .child(layer)
}

fn model_picker_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    for cell in &mut cells {
        cell.bg = BLACK;
    }
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "models", GRAY);
        write_at(&mut cells, width, 1, height / 2, "type to filter", TEXT);
        write_at(&mut cells, width, 1, height - 1, "enter · esc", DIM);
        return cells;
    }
    let model = app.model.borrow();
    let accent = accent(&model.prefs.theme);
    let card_width = 76.min(width.saturating_sub(4));
    let card_height = 24.min(height.saturating_sub(2)).max(8);
    let left = width.saturating_sub(card_width) / 2;
    let top = height.saturating_sub(card_height) / 2;
    for y in top..top + card_height {
        for x in left..left + card_width {
            if let Some(cell) = cells.get_mut(y * width + x) {
                *cell = Cell::blank();
            }
        }
    }
    write_at(
        &mut cells,
        width,
        left,
        top,
        &format!("╭{}╮", "─".repeat(card_width - 2)),
        accent,
    );
    write_at(&mut cells, width, left + 2, top, "models", accent);
    for y in top + 1..top + card_height - 1 {
        write_at(&mut cells, width, left, y, "│", accent);
        write_at(&mut cells, width, left + card_width - 1, y, "│", accent);
    }
    write_at(
        &mut cells,
        width,
        left,
        top + card_height - 1,
        &format!("╰{}╯", "─".repeat(card_width - 2)),
        accent,
    );
    let query_width = card_width.saturating_sub(14);
    let query = draft_view(
        &app.model_draft.peek(),
        app.model_caret.peek(),
        query_width,
        '▌',
    );
    write_at(&mut cells, width, left + 2, top + 2, "filter", DIM);
    write_at(&mut cells, width, left + 10, top + 2, &query, TEXT);
    write_at(
        &mut cells,
        width,
        left + 1,
        top + 4,
        &"─".repeat(card_width - 2),
        DIM,
    );
    let choices = app.filtered_models();
    let selected = app.model_sel.peek().min(choices.len().saturating_sub(1));
    let error = app.model_error.peek();
    let mut list_y = top + 5;
    if app.model_loading.peek() {
        write_at(&mut cells, width, left + 3, list_y, "loading models…", DIM);
    } else {
        if !error.is_empty() {
            write_at(
                &mut cells,
                width,
                left + 3,
                list_y,
                "couldn't load the host list — type an exact id",
                YELLOW,
            );
            write_at(
                &mut cells,
                width,
                left + 3,
                list_y + 1,
                &ellipsize(
                    &error.lines().collect::<Vec<_>>().join(" "),
                    card_width.saturating_sub(6),
                ),
                RED,
            );
            list_y += 2;
        } else if choices.is_empty() {
            write_at(
                &mut cells,
                width,
                left + 3,
                list_y,
                "no matching models — type an exact id",
                YELLOW,
            );
        }
        if !choices.is_empty() {
            let visible = card_height.saturating_sub(list_y.saturating_sub(top) + 2);
            let start = selected
                .saturating_sub(visible / 2)
                .min(choices.len().saturating_sub(visible));
            for (offset, (index, choice)) in choices
                .iter()
                .enumerate()
                .skip(start)
                .take(visible)
                .enumerate()
            {
                let current = match choice.split_once('\t') {
                    Some((name, _)) => name == model.snapshot.endpoint,
                    None => choice == &model.model,
                };
                let marker = if index == selected { '›' } else { ' ' };
                let active = if current { '●' } else { ' ' };
                let label = match choice.split_once('\t') {
                    Some((name, model)) => format!("{name:<12} {model}"),
                    None => choice.clone(),
                };
                write_at(
                    &mut cells,
                    width,
                    left + 3,
                    list_y + offset,
                    &format!(
                        "{marker} {active} {}",
                        ellipsize(&label, card_width.saturating_sub(10))
                    ),
                    if index == selected { TEXT } else { DIM },
                );
            }
        }
    }
    let hint = " ↑↓ select · type to filter · enter use · esc close ";
    write_at(
        &mut cells,
        width,
        left + card_width.saturating_sub(hint.chars().count() + 1),
        top + card_height - 1,
        hint,
        DIM,
    );
    cells
}

fn settings_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    for cell in &mut cells {
        cell.bg = BLACK;
    }
    if width < 8 || height < 3 {
        return cells;
    }
    if width < 20 {
        let labels = [
            "mode",
            "verify",
            "rounds",
            "endpoint",
            "model",
            "theme",
            "splash",
            "motion",
            "reasoning",
            "tools",
            "bus",
        ];
        let selected = labels[app.settings_sel.peek().min(labels.len() - 1)];
        write_at(&mut cells, width, 1, 0, "settings", GRAY);
        write_at(
            &mut cells,
            width,
            1,
            height / 2,
            &format!("› {selected}"),
            TEXT,
        );
        write_at(&mut cells, width, 1, height - 1, "↑↓ ←→ esc", DIM);
        return cells;
    }
    let model = app.model.borrow();
    let expanded = height >= 20;
    let card_width = 68.min(width.saturating_sub(4));
    let card_height = if expanded { 20 } else { 15.min(height) };
    let left = width.saturating_sub(card_width) / 2;
    let top = (height / 2).saturating_sub(card_height / 2);
    for y in top..top + card_height {
        for x in left..left + card_width {
            if let Some(cell) = cells.get_mut(y * width + x) {
                *cell = Cell::blank();
            }
        }
    }
    let accent = accent(&model.prefs.theme);
    write_at(
        &mut cells,
        width,
        left,
        top,
        &format!("╭{}╮", "─".repeat(card_width - 2)),
        accent,
    );
    write_at(&mut cells, width, left + 1, top, " ⚙ settings ", accent);
    for y in top + 1..top + card_height - 1 {
        write_at(&mut cells, width, left, y, "│", accent);
        write_at(&mut cells, width, left + card_width - 1, y, "│", accent);
    }
    write_at(
        &mut cells,
        width,
        left,
        top + card_height - 1,
        &format!("╰{}╯", "─".repeat(card_width - 2)),
        accent,
    );
    let hints: &[&str] = &[
        " tab/↑↓ select · ←→ change · enter open · esc close ",
        " ↑↓ select · ←→ change · esc close ",
        " ↑↓ · ←→ · enter · esc ",
        " ↑↓ ←→ esc ",
    ];
    let hint = hints
        .iter()
        .copied()
        .find(|hint| UnicodeWidthStr::width(*hint) <= card_width.saturating_sub(2))
        .unwrap_or("");
    write_at(
        &mut cells,
        width,
        left + card_width.saturating_sub(hint.chars().count() + 1),
        top + card_height - 1,
        hint,
        DIM,
    );
    let verify = model
        .snapshot
        .verify
        .as_deref()
        .map_or_else(|| "off".to_string(), |command| format!("on · {command}"));
    let endpoint = model.snapshot.relay.as_deref().map_or_else(
        || model.snapshot.endpoint.clone(),
        |relay| format!("{} · {relay}", model.snapshot.endpoint),
    );
    let rows = [
        (
            "mode",
            if model.snapshot.plan {
                "plan · read-only"
            } else {
                "auto"
            }
            .to_string(),
        ),
        ("verify", verify),
        ("rounds", model.snapshot.max_rounds.to_string()),
        ("endpoint", endpoint),
        ("model", model.snapshot.model.clone()),
        ("theme", model.prefs.theme.clone()),
        ("splash", on_off(model.prefs.splash).to_string()),
        ("motion", on_off(model.prefs.animations).to_string()),
        (
            "reasoning",
            if model.prefs.reasoning {
                "shown"
            } else {
                "hidden"
            }
            .to_string(),
        ),
        ("tools", model.prefs.tools.name().to_string()),
        (
            "bus",
            if model.prefs.bus {
                "mail starts a turn"
            } else {
                "mail waits"
            }
            .to_string(),
        ),
    ];
    let available = card_height.saturating_sub(2);
    let compact = !expanded;
    let start = if compact {
        app.settings_sel
            .peek()
            .saturating_sub(available / 2)
            .min(rows.len().saturating_sub(available))
    } else {
        0
    };
    let end = if compact {
        (start + available).min(rows.len())
    } else {
        rows.len()
    };
    if expanded {
        for (offset, label) in [(1, "session"), (6, "provider"), (10, "interface")] {
            write_at(&mut cells, width, left + 2, top + offset, label, DIM);
        }
    }
    let positions = [2, 3, 4, 7, 8, 11, 12, 13, 14, 15, 16];
    for (offset, (index, (label, value))) in
        rows.iter().enumerate().take(end).skip(start).enumerate()
    {
        let y = if expanded {
            top + positions[index]
        } else {
            top + 1 + offset
        };
        let selected = index == app.settings_sel.peek();
        let marker = if selected { accent } else { DIM };
        write_at(
            &mut cells,
            width,
            left + 2,
            y,
            &format!("{} {label:<11}", if selected { '›' } else { ' ' }),
            marker,
        );
        let value_width = card_width.saturating_sub(19);
        let value = ellipsize(value, value_width);
        write_at(
            &mut cells,
            width,
            left + 17,
            y,
            &value,
            if selected { TEXT } else { DIM },
        );
    }
    cells
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
