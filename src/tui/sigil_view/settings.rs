use sigil_tui::cell_width;

use super::app::App;
use super::canvas::{Cell, palette};
use super::chat::{accent, draft_view, ellipsize, row, write_at};
use super::hit::{self, Target};
use sigil::prelude::{
    IntoNode, LayoutBuilderExt, LayoutStyle, NodeBuilder, Position, Rgba8, container, signal,
    text_edit, text_field,
};

pub(super) fn frame(
    cells: &mut [Cell],
    width: usize,
    bounds: (usize, usize, usize, usize),
    color: Rgba8,
) {
    let (left, top, card_width, card_height) = bounds;
    write_at(
        cells,
        width,
        left,
        top,
        &format!("╭{}╮", "─".repeat(card_width - 2)),
        color,
    );
    for y in top + 1..top + card_height - 1 {
        write_at(cells, width, left, y, "│", color);
        write_at(cells, width, left + card_width - 1, y, "│", color);
    }
    write_at(
        cells,
        width,
        left,
        top + card_height - 1,
        &format!("╰{}╯", "─".repeat(card_width - 2)),
        color,
    );
}

pub(super) fn card(width: usize, height: usize, rows: usize) -> (usize, usize, usize, usize) {
    let card_width = (width * 4 / 5).max(60).min(width.saturating_sub(4));
    let card_height = rows.min(height.saturating_sub(2)).max(8.min(height));
    (
        width.saturating_sub(card_width) / 2,
        height.saturating_sub(card_height) / 2,
        card_width,
        card_height,
    )
}

pub(super) fn footer(
    cells: &mut [Cell],
    width: usize,
    bounds: (usize, usize, usize, usize),
    hints: &[&str],
) {
    let (left, top, card_width, card_height) = bounds;
    let hint = hints
        .iter()
        .copied()
        .find(|hint| cell_width(hint) <= card_width.saturating_sub(2))
        .unwrap_or("");
    write_at(
        cells,
        width,
        left + card_width.saturating_sub(cell_width(hint) + 1),
        top + card_height - 1,
        hint,
        palette().dim,
    );
}

pub(super) fn settings_layer(app: App) -> NodeBuilder {
    let input = text_field(signal(String::new())).class("w-full h-full");
    layer(app, settings_grid, input)
}

pub(super) fn hotkeys_layer(app: App) -> NodeBuilder {
    let input = text_field(signal(String::new())).class("w-full h-full");
    layer(app, hotkeys_grid, input)
}

pub(super) fn model_picker_layer(app: App) -> NodeBuilder {
    let changed = app.clone();
    let input = text_edit(app.model_draft)
        .caret_position(app.model_caret)
        .change(move |draft| changed.replace_model_draft(draft.to_string()))
        .class("w-full h-full");
    layer(app, model_picker_grid, input)
}

pub(super) fn workflow_layer(app: App) -> NodeBuilder {
    let input = text_edit(app.workflow_draft)
        .caret_position(app.workflow_caret)
        .class("w-full h-full");
    layer(app, workflow_grid, input)
}

fn workflow_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "workflow", palette().gray);
        write_at(
            &mut cells,
            width,
            1,
            height.saturating_sub(1),
            "enter · esc",
            palette().dim,
        );
        return cells;
    }
    let model = app.model.borrow();
    let accent = accent(&model.prefs.theme);
    let (left, top, card_width, card_height) = card(width, height, 15);
    frame(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        accent,
    );
    let name = app.workflow_name.peek();
    let title = if name.is_empty() {
        " ⚙ workflow (unsaved) ".to_string()
    } else {
        format!(" ⚙ workflow {name} ")
    };
    write_at(&mut cells, width, left + 1, top, &title, accent);
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
            if lit { accent } else { palette().dim },
        );
        let (value, color) = match seats.get(index).and_then(|seat| seat.as_deref()) {
            Some(seat) => (seat.replace('\t', "  "), palette().text),
            None => (driving.clone(), palette().dim),
        };
        write_at(
            &mut cells,
            width,
            left + 15,
            y,
            &ellipsize(&value, value_width),
            if index == selected {
                color
            } else {
                palette().dim
            },
        );
    }
    let names = app.workflow_names.peek();
    let saved_y = top + 9;
    write_at(
        &mut cells,
        width,
        left + 2,
        saved_y,
        "saved:",
        palette().dim,
    );
    let mut x = left + 9;
    if names.is_empty() {
        write_at(&mut cells, width, x, saved_y, "none", palette().dim);
    }
    for (index, candidate) in names.iter().enumerate() {
        let label = if *candidate == name {
            format!("[{candidate}]")
        } else {
            candidate.clone()
        };
        let span = cell_width(label.as_str());
        if x + span + 2 > left + card_width - 1 {
            write_at(&mut cells, width, x, saved_y, "…", palette().dim);
            break;
        }
        if index > 0 {
            write_at(&mut cells, width, x, saved_y, "· ", palette().dim);
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
            if *candidate == name {
                palette().text
            } else {
                palette().dim
            },
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
        write_at(&mut cells, width, left + 2, top + 11, "name", palette().dim);
        write_at(
            &mut cells,
            width,
            left + 9,
            top + 11,
            &draft,
            palette().text,
        );
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
    footer(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        hints,
    );
    cells
}

pub(super) fn auth_layer(app: App) -> NodeBuilder {
    let input = text_edit(app.auth_key)
        .caret_position(app.auth_caret)
        .class("w-full h-full");
    layer(app, auth_grid, input)
}

fn auth_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "authentication", palette().gray);
        write_at(
            &mut cells,
            width,
            1,
            height - 1,
            "enter · esc",
            palette().dim,
        );
        return cells;
    }
    let model = app.model.borrow();
    let accent = accent(&model.prefs.theme);
    let (left, top, card_width, card_height) = card(width, height, height);
    frame(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        accent,
    );
    write_at(&mut cells, width, left + 2, top, "authentication", accent);
    let choices = app.auth_choices.peek();
    let selected = app.auth_sel.peek().min(choices.len().saturating_sub(1));
    app.hits.clear();
    if app.auth_entering.peek() {
        let Some(choice) = choices.get(selected) else {
            return cells;
        };
        write_at(
            &mut cells,
            width,
            left + 3,
            top + 3,
            &choice.name,
            palette().text,
        );
        write_at(
            &mut cells,
            width,
            left + 3,
            top + 5,
            "API key",
            palette().dim,
        );
        let masked = format!("{}▌", "•".repeat(app.auth_key.peek().chars().count()));
        write_at(
            &mut cells,
            width,
            left + 13,
            top + 5,
            &ellipsize(&masked, card_width.saturating_sub(17)),
            palette().text,
        );
        write_at(
            &mut cells,
            width,
            left + 3,
            top + 8,
            "Saves in the credential store",
            palette().dim,
        );
        footer(
            &mut cells,
            width,
            (left, top, card_width, card_height),
            &[" enter save · esc back ", " enter · esc "],
        );
        return cells;
    }
    let visible = card_height.saturating_sub(4);
    let start = hit::window(app, selected, choices.len(), visible);
    let status_width = choices
        .iter()
        .map(|choice| cell_width(&choice.state()))
        .max()
        .unwrap_or(0);
    let name_width = 14.min(card_width.saturating_sub(status_width + 9));
    for (offset, (index, choice)) in choices
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let status = choice.state();
        let name = ellipsize(&choice.name, name_width);
        let padding = " ".repeat(name_width.saturating_sub(cell_width(&name)));
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
            &ellipsize(
                &format!("{marker} {name}{padding} {status}"),
                card_width.saturating_sub(6),
            ),
            if index == selected {
                palette().text
            } else {
                palette().dim
            },
        );
    }
    footer(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        &[
            " ↑↓ select · enter configure/login · esc close ",
            " ↑↓ · enter · esc ",
            " enter · esc ",
        ],
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
    if width == 0 || height == 0 {
        return cells;
    }
    if width < 20 || height < 8 {
        write_at(&mut cells, width, 1, 0, "models", palette().gray);
        write_at(
            &mut cells,
            width,
            1,
            height / 2,
            "type to filter",
            palette().text,
        );
        write_at(
            &mut cells,
            width,
            1,
            height.saturating_sub(1),
            "enter · esc",
            palette().dim,
        );
        return cells;
    }
    let model = app.model.borrow();
    let accent = accent(&model.prefs.theme);
    let (left, top, card_width, card_height) = card(width, height, height);
    frame(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        accent,
    );
    write_at(&mut cells, width, left + 2, top, "models", accent);
    let query_width = card_width.saturating_sub(14);
    let query = draft_view(
        &app.model_draft.peek(),
        app.model_caret.peek(),
        query_width,
        '▌',
    );
    write_at(
        &mut cells,
        width,
        left + 2,
        top + 2,
        "filter",
        palette().dim,
    );
    write_at(
        &mut cells,
        width,
        left + 10,
        top + 2,
        &query,
        palette().text,
    );
    write_at(
        &mut cells,
        width,
        left + 1,
        top + 4,
        &"─".repeat(card_width - 2),
        palette().dim,
    );
    let choices = app.filtered_models();
    let selected = app.model_sel.peek().min(choices.len().saturating_sub(1));
    let error = app.model_error.peek();
    let mut list_y = top + 5;
    app.hits.clear();
    if app.model_loading.peek() {
        write_at(
            &mut cells,
            width,
            left + 3,
            list_y,
            "loading models…",
            palette().dim,
        );
        list_y += 1;
    }
    if !error.is_empty() {
        write_at(
            &mut cells,
            width,
            left + 3,
            list_y,
            &ellipsize(
                &error.lines().collect::<Vec<_>>().join(" "),
                card_width.saturating_sub(6),
            ),
            palette().error,
        );
        list_y += 1;
    }
    if choices.is_empty() && !app.model_loading.peek() {
        write_at(
            &mut cells,
            width,
            left + 3,
            list_y,
            &ellipsize(
                "No matches. Change the filter.",
                card_width.saturating_sub(6),
            ),
            palette().yellow,
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
        let start = hit::window(app, selected_row, rows.len(), visible);
        for (offset, row) in rows.iter().skip(start).take(visible).enumerate() {
            let y = list_y + offset;
            match row {
                ModelRow::Header(label) => {
                    write_at(&mut cells, width, left + 3, y, label, palette().dim);
                }
                ModelRow::Seat(index) => {
                    let choice = &choices[*index];
                    let current = model.is_model(choice);
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
                        if *index == selected {
                            palette().text
                        } else {
                            palette().dim
                        },
                    );
                }
            }
        }
    }
    footer(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        &[
            " ↑↓ select · type to filter · enter use · esc close ",
            " ↑↓ · enter use · esc close ",
            " enter · esc ",
        ],
    );
    cells
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Setting {
    Mode,
    Verify,
    Workflow,
    Endpoint,
    Model,
    Reasoning,
    Tools,
    Bus,
    Comments,
    Tests,
    Module(usize),
    Theme,
    Splash,
    Motion,
    Receipts,
    Hotkeys,
}

pub(super) struct Row {
    pub(super) group: &'static str,
    pub(super) setting: Setting,
    pub(super) label: String,
    pub(super) value: String,
}

pub(super) fn setting_rows(app: &App) -> Vec<Row> {
    let model = app.model.borrow();
    let snapshot = &model.snapshot;
    let prefs = &model.prefs;
    let mode = if snapshot.plan {
        "plan · read-only"
    } else if snapshot.work {
        "work · execute the plan"
    } else if snapshot.goal {
        "goal · loop until met"
    } else {
        "auto"
    };
    let mut rows = vec![
        ("session", Setting::Mode, "mode", mode.to_string()),
        (
            "session",
            Setting::Verify,
            "verify",
            snapshot
                .verify
                .as_deref()
                .map_or_else(|| "off".to_string(), |command| format!("on · {command}")),
        ),
        (
            "session",
            Setting::Workflow,
            "workflow",
            snapshot
                .workflow
                .clone()
                .unwrap_or_else(|| "off".to_string()),
        ),
        (
            "model",
            Setting::Endpoint,
            "endpoint",
            snapshot.relay.as_deref().map_or_else(
                || snapshot.endpoint.clone(),
                |relay| format!("{} · {relay}", snapshot.endpoint),
            ),
        ),
        ("model", Setting::Model, "model", snapshot.model.clone()),
        (
            "model",
            Setting::Reasoning,
            "reasoning",
            prefs.reasoning.name().to_string(),
        ),
        (
            "agent",
            Setting::Tools,
            "tools",
            prefs.tools.name().to_string(),
        ),
        (
            "agent",
            Setting::Bus,
            "bus",
            if prefs.bus {
                "mail starts a turn"
            } else {
                "mail waits"
            }
            .to_string(),
        ),
        (
            "agent",
            Setting::Comments,
            "comments",
            banned(prefs.ban_comments).to_string(),
        ),
        (
            "agent",
            Setting::Tests,
            "tests",
            banned(prefs.ban_tests).to_string(),
        ),
    ];
    let modules = app.modules.peek();
    for (index, module) in crate::modules::ALL.iter().enumerate() {
        rows.push((
            "modules",
            Setting::Module(index),
            module.name,
            on_off(modules.get(module.name).copied().unwrap_or(module.default)).to_string(),
        ));
    }
    rows.extend([
        ("interface", Setting::Theme, "theme", prefs.theme.clone()),
        (
            "interface",
            Setting::Splash,
            "splash",
            on_off(prefs.splash).to_string(),
        ),
        (
            "interface",
            Setting::Motion,
            "motion",
            on_off(prefs.animations).to_string(),
        ),
        (
            "interface",
            Setting::Receipts,
            "receipts",
            prefs.receipts.name().to_string(),
        ),
        (
            "interface",
            Setting::Hotkeys,
            "hotkeys",
            format!("{} bindings", crate::prefs::Hotkey::ALL.len()),
        ),
    ]);
    rows.into_iter()
        .map(|(group, setting, label, value)| Row {
            group,
            setting,
            label: label.to_string(),
            value,
        })
        .collect()
}

fn settings_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    if width < 8 || height < 3 {
        return cells;
    }
    let rows = setting_rows(app);
    let selected = app.settings_sel.peek().min(rows.len().saturating_sub(1));
    if width < 20 {
        write_at(&mut cells, width, 1, 0, "options", palette().gray);
        write_at(
            &mut cells,
            width,
            1,
            height / 2,
            &format!("› {}", rows[selected].label),
            palette().text,
        );
        write_at(&mut cells, width, 1, height - 1, "↑↓ ←→ esc", palette().dim);
        return cells;
    }
    let mut lines: Vec<Option<usize>> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if index == 0 || rows[index - 1].group != row.group {
            if index > 0 {
                lines.push(None);
            }
            lines.push(None);
        }
        lines.push(Some(index));
    }
    let model = app.model.borrow();
    let (left, top, card_width, card_height) = card(width, height, lines.len() + 2);
    let accent = accent(&model.prefs.theme);
    frame(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        accent,
    );
    write_at(&mut cells, width, left + 1, top, " ⚙ options ", accent);
    let hints: &[&str] = &[
        " ↑↓ select · ←→ change · enter open · esc close ",
        " ↑↓ · ←→ · enter · esc ",
        " ↑↓ ←→ esc ",
    ];
    footer(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        hints,
    );
    let available = card_height.saturating_sub(2).max(1);
    let line = lines
        .iter()
        .position(|entry| *entry == Some(selected))
        .unwrap_or(0);
    let start = hit::window(app, line, lines.len(), available);
    app.hits.clear();
    for (offset, entry) in lines.iter().enumerate().skip(start).take(available) {
        let y = top + 1 + (offset - start);
        let Some(index) = *entry else {
            if let Some(Some(next)) = lines.get(offset + 1) {
                write_at(
                    &mut cells,
                    width,
                    left + 2,
                    y,
                    rows[*next].group,
                    palette().gray,
                );
            }
            continue;
        };
        let row = &rows[index];
        let active = index == selected;
        app.hits
            .add(y, left + 1, left + card_width - 1, Target::Setting(index));
        write_at(
            &mut cells,
            width,
            left + 2,
            y,
            &format!("{} {:<11}", if active { '›' } else { ' ' }, row.label),
            if active { accent } else { palette().dim },
        );
        write_at(
            &mut cells,
            width,
            left + 17,
            y,
            &ellipsize(&row.value, card_width.saturating_sub(19)),
            if active {
                palette().text
            } else {
                palette().dim
            },
        );
    }
    cells
}

fn hotkeys_grid(app: &App, width: usize, height: usize) -> Vec<Cell> {
    use crate::prefs::Hotkey;

    let mut cells = vec![Cell::blank(); width.saturating_mul(height)];
    if width < 8 || height < 3 {
        return cells;
    }
    let selected = app.hotkey_sel.peek().min(Hotkey::ALL.len() - 1);
    let model = app.model.borrow();
    if width < 28 || height < Hotkey::ALL.len() + 4 {
        let action = Hotkey::ALL[selected];
        write_at(&mut cells, width, 1, 0, "hotkeys", palette().gray);
        write_at(
            &mut cells,
            width,
            1,
            height / 2,
            &ellipsize(
                &format!("› {}  {}", action.name(), model.prefs.hotkeys.label(action)),
                width.saturating_sub(2),
            ),
            palette().text,
        );
        write_at(
            &mut cells,
            width,
            1,
            height - 1,
            "↑↓ enter esc",
            palette().dim,
        );
        return cells;
    }
    let accent = accent(&model.prefs.theme);
    let (left, top, card_width, card_height) = card(width, height, Hotkey::ALL.len() + 4);
    frame(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        accent,
    );
    write_at(&mut cells, width, left + 1, top, " hotkeys ", accent);
    let hints: &[&str] = if app.hotkey_recording.peek() {
        &[" press ctrl+letter · esc cancel ", " ctrl+letter · esc "]
    } else {
        &[
            " ↑↓ select · enter change · r reset · esc back ",
            " ↑↓ · enter · r · esc ",
        ]
    };
    footer(
        &mut cells,
        width,
        (left, top, card_width, card_height),
        hints,
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
            if active { accent } else { palette().dim },
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
            if active {
                palette().text
            } else {
                palette().dim
            },
        );
        write_at(
            &mut cells,
            width,
            left + 36,
            y,
            hotkey_description(action),
            palette().dim,
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
            palette().error,
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
        Hotkey::Search => "search conversation",
        Hotkey::History => "search saved prompts",
    }
}

const fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

const fn banned(value: bool) -> &'static str {
    if value { "banned" } else { "allowed" }
}

pub(super) fn layer(
    app: App,
    grid: fn(&App, usize, usize) -> Vec<Cell>,
    input: impl IntoNode + 'static,
) -> NodeBuilder {
    let paint = app.clone();
    let overlay = container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset(0.0)
                .fill_width()
                .fill_height()
                .gap(0.0),
        )
        .children_reactive(move || {
            super::view::watch(&paint);
            let _revision = paint.panes.get();
            let (cols, rows) = paint.viewport.get();
            grid(&paint, usize::from(cols), usize::from(rows))
                .chunks(usize::from(cols).max(1))
                .map(row)
                .collect::<Vec<_>>()
        });
    container()
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(input)
        .child(hit::wire(&app, overlay))
}
