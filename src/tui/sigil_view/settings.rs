use sigil::prelude::*;
use unicode_width::UnicodeWidthStr;

use super::app::App;
use super::canvas::{Cell, DIM, GRAY, TEXT};
use super::chat::{BLACK, RED, YELLOW, accent, draft_view, ellipsize, row, write_at};
use super::hit::{self, Target};

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
        .child(hit::wire(&app, layer))
}

pub(super) fn hotkeys_layer(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let cells = hotkeys_grid(&app, usize::from(cols), usize::from(rows));
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
        .child(hit::wire(&app, layer))
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
        .child(hit::wire(&app, layer))
}

pub(super) fn workflow_layer(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let cells = workflow_grid(&app, usize::from(cols), usize::from(rows));
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
            text_edit(app.workflow_draft)
                .caret_position(app.workflow_caret)
                .class("w-full h-full"),
        )
        .child(hit::wire(&app, layer))
}

fn workflow_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    for cell in &mut cells {
        cell.bg = BLACK;
    }
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "workflow", GRAY);
        write_at(
            &mut cells,
            width,
            1,
            height.saturating_sub(1),
            "enter · esc",
            DIM,
        );
        return cells;
    }
    let model = app.model.borrow();
    let accent = accent(&model.prefs.theme);
    let card_width = 72.min(width.saturating_sub(4));
    let card_height = 15.min(height.saturating_sub(2)).max(8);
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
    let name = app.workflow_name.peek();
    let title = if name.is_empty() {
        " ⚙ workflow (unsaved) ".to_string()
    } else {
        format!(" ⚙ workflow {name} ")
    };
    write_at(&mut cells, width, left + 1, top, &title, accent);
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
    let seats = app.workflow_seats.peek();
    let selected = app.workflow_sel.peek();
    let dragging = app.drag_from.peek();
    let over = app.drag_over.peek();
    let driving = format!(
        "driving · {} {}",
        model.snapshot.endpoint, model.snapshot.model
    );
    let value_width = card_width.saturating_sub(18);
    app.hits.clear();
    for (index, role) in crate::workflow::Role::ALL.iter().enumerate() {
        let y = top + 2 + index;
        app.hits
            .add(y, left + 1, left + card_width - 1, Target::Role(index));
        let marker = match (dragging, over) {
            (Some(from), _) if from == index => '◆',
            (Some(from), Some(to)) if to == index && to != from => '▸',
            _ if index == selected => '›',
            _ => ' ',
        };
        let lit = index == selected || over == Some(index);
        write_at(
            &mut cells,
            width,
            left + 2,
            y,
            &format!("{marker} {role:<10}"),
            if lit { accent } else { DIM },
        );
        let (value, color) = match seats.get(index).and_then(|seat| seat.as_deref()) {
            Some(seat) => (seat.replace('\t', "  "), TEXT),
            None => (driving.clone(), DIM),
        };
        write_at(
            &mut cells,
            width,
            left + 15,
            y,
            &ellipsize(&value, value_width),
            if index == selected { color } else { DIM },
        );
    }
    let names = app.workflow_names.peek();
    let saved_y = top + 9;
    write_at(&mut cells, width, left + 2, saved_y, "saved:", DIM);
    let mut x = left + 9;
    if names.is_empty() {
        write_at(&mut cells, width, x, saved_y, "none", DIM);
    }
    for (index, candidate) in names.iter().enumerate() {
        let label = if *candidate == name {
            format!("[{candidate}]")
        } else {
            candidate.clone()
        };
        let span = UnicodeWidthStr::width(label.as_str());
        if x + span + 2 > left + card_width - 1 {
            write_at(&mut cells, width, x, saved_y, "…", DIM);
            break;
        }
        if index > 0 {
            write_at(&mut cells, width, x, saved_y, "· ", DIM);
            x += 2;
        }
        app.hits
            .add(saved_y, x, x + span, Target::Saved(candidate.clone()));
        write_at(
            &mut cells,
            width,
            x,
            saved_y,
            &label,
            if *candidate == name { TEXT } else { DIM },
        );
        x += span + 1;
    }
    if app.workflow_naming.peek() {
        let draft = draft_view(
            &app.workflow_draft.peek(),
            app.workflow_caret.peek(),
            card_width.saturating_sub(12),
            '▌',
        );
        write_at(&mut cells, width, left + 2, top + 11, "name", DIM);
        write_at(&mut cells, width, left + 9, top + 11, &draft, TEXT);
    }
    let hints: &[&str] = if app.workflow_naming.peek() {
        &[" enter save · esc back ", " enter · esc "]
    } else if dragging.is_some() {
        &[" drop on a role to copy the seat ", " drop to copy "]
    } else {
        &[
            " ↑↓ role · enter seat · d driving · ←→ saved · s save · drag to copy · esc ",
            " ↑↓ role · enter seat · d driving · ←→ saved · s save · esc ",
            " ↑↓ · enter · d · ←→ · s · esc ",
            " enter · esc ",
        ]
    };
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
    cells
}

pub(super) fn auth_layer(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let cells = auth_grid(&app, usize::from(cols), usize::from(rows));
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
            text_edit(app.auth_key)
                .caret_position(app.auth_caret)
                .class("w-full h-full"),
        )
        .child(hit::wire(&app, layer))
}

fn auth_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    for cell in &mut cells {
        cell.bg = BLACK;
    }
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "authentication", GRAY);
        write_at(&mut cells, width, 1, height - 1, "enter · esc", DIM);
        return cells;
    }
    let model = app.model.borrow();
    let accent = accent(&model.prefs.theme);
    let card_width = 70.min(width.saturating_sub(4));
    let card_height = 20.min(height.saturating_sub(2)).max(8);
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
    write_at(&mut cells, width, left + 2, top, "authentication", accent);
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
    let choices = app.auth_choices.peek();
    let selected = app.auth_sel.peek().min(choices.len().saturating_sub(1));
    app.hits.clear();
    if app.auth_entering.peek() {
        let Some(choice) = choices.get(selected) else {
            return cells;
        };
        write_at(&mut cells, width, left + 3, top + 3, &choice.name, TEXT);
        write_at(&mut cells, width, left + 3, top + 5, "API key", DIM);
        let masked = format!("{}▌", "•".repeat(app.auth_key.peek().chars().count()));
        write_at(
            &mut cells,
            width,
            left + 13,
            top + 5,
            &ellipsize(&masked, card_width.saturating_sub(17)),
            TEXT,
        );
        write_at(
            &mut cells,
            width,
            left + 3,
            top + 8,
            "Saved to ~/.mage/config.toml. The key is not added to the session.",
            DIM,
        );
        let hint = " enter save · esc back ";
        write_at(
            &mut cells,
            width,
            left + card_width.saturating_sub(hint.chars().count() + 1),
            top + card_height - 1,
            hint,
            DIM,
        );
        return cells;
    }
    let visible = card_height.saturating_sub(4);
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
        let status = if choice.device_login {
            "device login"
        } else if choice.configured {
            "key configured"
        } else {
            "key missing"
        };
        let marker = if index == selected { '›' } else { ' ' };
        app.hits.add(
            top + 2 + offset,
            left + 1,
            left + card_width - 1,
            Target::Auth(index),
        );
        write_at(
            &mut cells,
            width,
            left + 3,
            top + 2 + offset,
            &format!("{marker} {:<14} {status}", choice.name),
            if index == selected { TEXT } else { DIM },
        );
    }
    let hint = " ↑↓ select · enter configure/login · esc close ";
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

enum ModelRow {
    Header(&'static str),
    Seat(usize),
}

fn list_rows(count: usize, recent: usize, sections: bool) -> Vec<ModelRow> {
    if !sections {
        return (0..count).map(ModelRow::Seat).collect();
    }
    let mut rows = Vec::with_capacity(count + 2);
    rows.push(ModelRow::Header("recent"));
    rows.extend((0..recent).map(ModelRow::Seat));
    if recent < count {
        rows.push(ModelRow::Header("providers"));
        rows.extend((recent..count).map(ModelRow::Seat));
    }
    rows
}

fn model_picker_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    for cell in &mut cells {
        cell.bg = BLACK;
    }
    if width == 0 || height == 0 {
        return cells;
    }
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "models", GRAY);
        write_at(&mut cells, width, 1, height / 2, "type to filter", TEXT);
        write_at(
            &mut cells,
            width,
            1,
            height.saturating_sub(1),
            "enter · esc",
            DIM,
        );
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
    app.hits.clear();
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
            let recent = app.model_recent.peek().min(choices.len());
            let sections = app.model_draft.peek().is_empty() && recent > 0;
            let rows = list_rows(choices.len(), recent, sections);
            let selected_row = rows
                .iter()
                .position(|row| matches!(row, ModelRow::Seat(index) if *index == selected))
                .unwrap_or(0);
            let visible = card_height.saturating_sub(list_y.saturating_sub(top) + 2);
            let start = selected_row
                .saturating_sub(visible / 2)
                .min(rows.len().saturating_sub(visible));
            for (offset, row) in rows.iter().skip(start).take(visible).enumerate() {
                let y = list_y + offset;
                match row {
                    ModelRow::Header(label) => {
                        write_at(&mut cells, width, left + 3, y, label, DIM);
                    }
                    ModelRow::Seat(index) => {
                        let choice = &choices[*index];
                        let current = match choice.split_once('\t') {
                            Some((name, _)) => name == model.snapshot.endpoint,
                            None => choice == &model.model,
                        };
                        let marker = if *index == selected { '›' } else { ' ' };
                        let active = if current { '●' } else { ' ' };
                        let label = match choice.split_once('\t') {
                            Some((name, model)) => format!("{name:<12} {model}"),
                            None => choice.clone(),
                        };
                        app.hits
                            .add(y, left + 1, left + card_width - 1, Target::Model(*index));
                        write_at(
                            &mut cells,
                            width,
                            left + 3,
                            y,
                            &format!(
                                "{marker} {active} {}",
                                ellipsize(&label, card_width.saturating_sub(10))
                            ),
                            if *index == selected { TEXT } else { DIM },
                        );
                    }
                }
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
            "workflow",
            "theme",
            "splash",
            "motion",
            "reasoning",
            "tools",
            "bus",
            "hotkeys",
        ];
        let selected = labels[app.settings_sel.peek().min(labels.len() - 1)];
        write_at(&mut cells, width, 1, 0, "options", GRAY);
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
    let expanded = height >= 21;
    let card_width = 68.min(width.saturating_sub(4));
    let card_height = if expanded { 21 } else { 15.min(height) };
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
    write_at(&mut cells, width, left + 1, top, " ⚙ options ", accent);
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
        " ↑↓ select · ←→ change · enter open · esc close ",
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
        ("rounds", model.snapshot.rounds_label()),
        ("endpoint", endpoint),
        ("model", model.snapshot.model.clone()),
        (
            "workflow",
            model
                .snapshot
                .workflow
                .clone()
                .unwrap_or_else(|| "off".to_string()),
        ),
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
        (
            "hotkeys",
            format!("{} bindings", crate::prefs::Hotkey::ALL.len()),
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
        for (offset, label) in [(1, "session"), (6, "provider"), (11, "interface")] {
            write_at(&mut cells, width, left + 2, top + offset, label, DIM);
        }
    }
    let positions = [2, 3, 4, 7, 8, 9, 12, 13, 14, 15, 16, 17, 18];
    app.hits.clear();
    for (offset, (index, (label, value))) in
        rows.iter().enumerate().take(end).skip(start).enumerate()
    {
        let y = if expanded {
            top + positions[index]
        } else {
            top + 1 + offset
        };
        app.hits
            .add(y, left + 1, left + card_width - 1, Target::Setting(index));
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

fn hotkeys_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    use crate::prefs::Hotkey;

    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    for cell in &mut cells {
        cell.bg = BLACK;
    }
    if width < 8 || height < 3 {
        return cells;
    }
    let selected = app.hotkey_sel.peek().min(Hotkey::ALL.len() - 1);
    let model = app.model.borrow();
    if width < 28 || height < 13 {
        let action = Hotkey::ALL[selected];
        write_at(&mut cells, width, 1, 0, "hotkeys", GRAY);
        write_at(
            &mut cells,
            width,
            1,
            height / 2,
            &ellipsize(
                &format!("› {}  {}", action.name(), model.prefs.hotkeys.label(action)),
                width.saturating_sub(2),
            ),
            TEXT,
        );
        write_at(&mut cells, width, 1, height - 1, "↑↓ enter esc", DIM);
        return cells;
    }
    let accent = accent(&model.prefs.theme);
    let card_width = 64.min(width.saturating_sub(4));
    let card_height = 13;
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
    write_at(&mut cells, width, left + 1, top, " hotkeys ", accent);
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
    let hint = if app.hotkey_recording.peek() {
        " press ctrl+letter · esc cancel "
    } else {
        " ↑↓ select · enter change · r reset · esc back "
    };
    write_at(
        &mut cells,
        width,
        left + card_width.saturating_sub(hint.chars().count() + 1),
        top + card_height - 1,
        hint,
        DIM,
    );
    app.hits.clear();
    for (index, action) in Hotkey::ALL.iter().copied().enumerate() {
        let y = top + 2 + index;
        app.hits
            .add(y, left + 1, left + card_width - 1, Target::Hotkey(index));
        let active = index == selected;
        write_at(
            &mut cells,
            width,
            left + 2,
            y,
            &format!("{} {:<10}", if active { '›' } else { ' ' }, action.name()),
            if active { accent } else { DIM },
        );
        let binding = if active && app.hotkey_recording.peek() {
            "press ctrl+letter".to_string()
        } else {
            model.prefs.hotkeys.label(action)
        };
        write_at(
            &mut cells,
            width,
            left + 16,
            y,
            &binding,
            if active { TEXT } else { DIM },
        );
        write_at(
            &mut cells,
            width,
            left + 36,
            y,
            hotkey_description(action),
            DIM,
        );
    }
    let note = app.hotkey_note.peek();
    if !note.is_empty() {
        write_at(
            &mut cells,
            width,
            left + 2,
            top + 10,
            &ellipsize(&note, card_width.saturating_sub(4)),
            RED,
        );
    }
    cells
}

const fn hotkey_description(action: crate::prefs::Hotkey) -> &'static str {
    use crate::prefs::Hotkey;

    match action {
        Hotkey::Options => "open options",
        Hotkey::Newline => "insert newline",
        Hotkey::Editor => "open $EDITOR",
        Hotkey::Files => "toggle files",
        Hotkey::Ledger => "toggle ledger",
        Hotkey::Diff => "toggle diff",
        Hotkey::Copy => "copy last reply",
        Hotkey::Select => "select transcript text",
        Hotkey::Pane => "hide/show pane",
    }
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}
