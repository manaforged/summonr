use sigil::prelude::Length;
use sigil::prelude::Paint;
use sigil::prelude::Space;
use sigil::prelude::{
    AlignItems, ListRowSlots, MouseButton, NodeBuilder, Tone, button, chip, list_row, node, spacer,
    text_edit,
};

use std::collections::BTreeSet;

use super::body::{Empty, Extra, header, listing};
use super::{App, Pane, PaneRow};
use crate::app::armed;
use crate::canvas;
use sigil::prelude::context_menu;

const ACCEPT: &str = "Accept";
const REVERT: &str = "Revert";
const NOTE: &str = "Note";
const UNDO: &str = "Undo";
const REVERT_FILE: &str = "Revert file";
const CONFIRM_REVERT: &str = "Confirm revert";
const SCOPE_ALL: &str = "Last turn only";
const SESSION_OFF: &str = "This session";
const SESSION_ON: &str = "All changes";
const SCOPE_RECENT: &str = "Every turn";
const NO_CHANGES: Empty = Empty {
    window: "No changes yet. Files the agent edits show here while the turn runs.",
    terminal: "no changes",
};
use crate::ui::{Hints, Key, input_keys};
use sigil::prelude::Widget;

pub(super) fn diff(app: App, mut column: NodeBuilder, cols: u16) -> NodeBuilder {
    let model = app.model.get();
    if let Some(row) = super::tree_actions::review(&app) {
        column = column.child(row);
    }
    if let Some(error) = model.diff_error.clone().filter(|_| canvas::is_window()) {
        return column.child(
            sigil::prelude::text(error)
                .wrap()
                .fill_width()
                .min_width(0.0)
                .min_height(14.0)
                .font_size(
                    12.0,
                    "12 px chrome text that keeps its own line height and weight",
                )
                .text_color(sigil::prelude::Token::Error11),
        );
    }
    if let Some(path) = app.diff_view.get() {
        let back = app.clone();
        column = column
            .child(header(
                &app,
                path.clone(),
                format!("vs {}", model.diff.base),
            ))
            .child(button("← all changes").ghost().activate(move || {
                back.pane_back();
            }));
        let Some((_, result)) = model.patch.as_ref().filter(|(seen, _)| *seen == path) else {
            return column.child(
                sigil::prelude::text("loading…")
                    .nowrap()
                    .font_size(
                        12.0,
                        "12 px chrome text that keeps its own line height and weight",
                    )
                    .text_color(sigil::prelude::Token::Gray11),
            );
        };
        let text = match result {
            Ok(text) => text,
            Err(error) => {
                return column.child(
                    sigil::prelude::text(error.clone())
                        .wrap()
                        .fill_width()
                        .min_width(0.0)
                        .min_height(14.0)
                        .font_size(
                            12.0,
                            "12 px chrome text that keeps its own line height and weight",
                        )
                        .text_color(sigil::prelude::Token::Error11),
                );
            }
        };
        let hunks = app.hunks();
        if text.trim().is_empty() {
            column = column.child(
                sigil::prelude::text("no changes left in this file")
                    .nowrap()
                    .font_size(
                        12.0,
                        "12 px chrome text that keeps its own line height and weight",
                    )
                    .text_color(sigil::prelude::Token::Gray11),
            );
            return undo_rows(&app, column, Some(&path), &BTreeSet::new());
        }
        if hunks.is_empty() {
            return column.child(crate::chat::entry::patch_view(
                &app,
                text.clone(),
                app.model.get().prefs.diff_layout,
                crate::chat::entry::Fit::of(cols),
            ));
        }
        let cursor = app.hunk_list.cursor.get();
        let notes = format!(
            "{} · {}",
            summonr_core::text::plural(hunks.len(), "hunk"),
            Hints::new()
                .key(Key::Fixed("a"), "accept")
                .key(Key::Fixed("r"), "revert")
                .key(Key::Fixed("n"), "note")
                .text(&model.prefs.hotkeys)
        );
        column = column.child(
            sigil::prelude::text(notes)
                .wrap()
                .fill_width()
                .min_width(0.0)
                .min_height(14.0)
                .font_size(
                    12.0,
                    "12 px chrome text that keeps its own line height and weight",
                )
                .text_color(sigil::prelude::Token::Gray11),
        );
        drop(model);
        for (index, hunk) in hunks.iter().enumerate() {
            column = column.child(hunk_view(
                &app,
                &path,
                index,
                hunk,
                (index == cursor, crate::chat::entry::Fit::of(cols)),
            ));
        }
        return undo_rows(&app, column, Some(&path), &BTreeSet::new());
    }
    let window = canvas::is_window();
    if window {
        column = column.child(scope(&app, &model.diff));
    } else {
        column = column.child(header(&app, "Changes".to_string(), model.diff.summary()));
    }
    drop(model);
    let listed: BTreeSet<String> = app.diff_rows().into_iter().map(|row| row.path).collect();
    let below = window.then_some(file_actions as Extra);
    column = listing(&app, Pane::Diff, column, &NO_CHANGES, (None, below));
    undo_rows(&app, column, None, &listed)
}

fn actions() -> NodeBuilder {
    sigil::prelude::row()
        .fill_width()
        .gap(Space::raw(6.0, "between the small and medium steps"))
        .align_items(AlignItems::Center)
}

fn hunk_actions(app: &App, path: &str, hunk: &str) -> NodeBuilder {
    let target = (path.to_string(), hunk.to_string());
    let (accept, revert, note) = (app.clone(), app.clone(), app.clone());
    let (a, r, n) = (target.clone(), target.clone(), target);
    actions()
        .child(
            button(ACCEPT)
                .ghost()
                .activate(move || accept.hunk_toggle(&a.0, &a.1)),
        )
        .child(
            button(REVERT)
                .ghost()
                .activate(move || revert.hunk_revert(&r.0, &r.1)),
        )
        .child(
            button(NOTE)
                .ghost()
                .activate(move || note.note_start(n.0.clone(), n.1.clone())),
        )
}

fn file_actions(app: &App, changed: &PaneRow) -> NodeBuilder {
    let path = changed.path.as_str();
    let (accepted, reverted) = app.file_review(path);
    let revert = app.clone();
    let target = path.to_string();
    let mut row = actions();
    if accepted > 0 {
        row = row.child(chip(format!("{accepted} accepted"), Tone::Success));
    }
    if reverted > 0 {
        row = row.child(chip(format!("{reverted} reverted"), Tone::Warning));
    }
    let label = if armed(&app.revert_armed, &target) {
        CONFIRM_REVERT
    } else {
        REVERT_FILE
    };
    row.child(spacer()).child(
        button(label)
            .ghost()
            .activate(move || revert.file_revert(&target)),
    )
}

fn undo_rows(
    app: &App,
    mut column: NodeBuilder,
    only: Option<&str>,
    skip: &BTreeSet<String>,
) -> NodeBuilder {
    if !canvas::is_window() {
        return column;
    }
    for gone in app
        .reverted(only)
        .into_iter()
        .filter(|gone| !skip.contains(&gone.path))
    {
        let undo = app.clone();
        let label = match only {
            Some(_) => format!("reverted {}", gone.header),
            None => format!("reverted {} {}", gone.path, gone.header),
        };
        column = column.child(
            actions()
                .child(
                    sigil::prelude::text(label)
                        .nowrap()
                        .ellipsize()
                        .grow(1.0)
                        .shrink(1.0)
                        .min_width(0.0)
                        .min_height(14.0)
                        .font_size(
                            12.0,
                            "12 px chrome text that keeps its own line height and weight",
                        )
                        .text_color(sigil::prelude::Token::Gray11),
                )
                .child(
                    button(UNDO)
                        .ghost()
                        .activate(move || undo.hunk_restore(&gone.path, &gone.hunk)),
                ),
        );
    }
    column
}

/// The panel's first row: how many files changed, and which changes it shows.
fn scope(app: &App, diff: &summonr_agent::diff::Diff) -> NodeBuilder {
    let count = sigil::prelude::text(summonr_core::text::plural(diff.files.len(), "file"))
        .nowrap()
        .font_size(13.0, "13 px between caption and body for dense code rows")
        .text_color(sigil::prelude::Token::OnSurfaceMuted);
    let toggle = app.clone();
    let label = if app.diff_recent.get() {
        SCOPE_RECENT
    } else {
        SCOPE_ALL
    };
    let session = app.clone();
    let whole = if app.diff_session.get() {
        SESSION_ON
    } else {
        SESSION_OFF
    };
    actions()
        .child(count)
        .child(spacer())
        .child(
            button(label)
                .ghost()
                .size(sigil::prelude::ControlSize::Compact)
                .activate(move || toggle.toggle_recent()),
        )
        .child(
            button(whole)
                .ghost()
                .size(sigil::prelude::ControlSize::Compact)
                .activate(move || session.toggle_session()),
        )
}

fn hunk_view(
    app: &App,
    path: &str,
    index: usize,
    hunk: &summonr_agent::hunks::Hunk,
    (focused, fit): (bool, crate::chat::entry::Fit),
) -> NodeBuilder {
    let note = app.hunk_note(path, &hunk.id);
    let (verdict, tone) = match note.as_ref().and_then(|note| note.verdict) {
        Some(summonr_agent::hunks::Verdict::Accepted) => ("accepted", Tone::Success),
        Some(summonr_agent::hunks::Verdict::Rejected) => ("reverted", Tone::Warning),
        None => ("pending", Tone::Neutral),
    };
    let editing = app
        .note_target
        .get()
        .is_some_and(|(target, id)| target == path && id == hunk.id);
    let (menu, items) = (app.clone(), app.clone());
    let mut card = sigil::prelude::column()
        .fill_width()
        .gap(Space::Xs)
        .child(context_menu(
            list_row(focused, hunk.header.clone(), ListRowSlots::default())
                .fill_width()
                .height(24.0)
                .padding_x(Space::Sm)
                .gap(Space::raw(6.0, "between the small and medium steps"))
                .align_items(AlignItems::Center)
                .child(chip(verdict, tone))
                .child(spacer())
                .pointer_down(move |event| {
                    menu.hunk_list.select(index);
                    menu.focus_pane(true);
                    if event.button != Some(MouseButton::Right) {
                        event.stop_propagation();
                    }
                }),
            move || items.hunk_menu(index),
        ));
    if canvas::is_window() && !editing {
        card = card.child(hunk_actions(app, path, &hunk.id));
    }
    if editing {
        let commit = app.clone();
        let edit = app.clone();
        card = card.child(node(move || {
            input_keys(&edit, text_edit(edit.note_text))
                .caret_position(edit.note_caret)
                .submit(move |text| commit.note_commit(text))
                .autofocus()
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .height(Length::Auto)
                .min_width(0.0)
                .min_height(20.0)
                .font_family(sigil::prelude::FontFamily::Mono)
                .font_size(
                    12.0,
                    "12 px chrome text that keeps its own line height and weight",
                )
                .background(sigil::prelude::Token::Gray1)
                .radius(sigil::prelude::Radius::Md)
        }));
        card = card.child(
            sigil::prelude::text(format!(
                "{} · sent with your next prompt",
                Hints::new()
                    .key(Key::Enter, "save")
                    .key(Key::Esc, "cancel")
                    .text(&app.model.get().prefs.hotkeys)
            ))
            .wrap()
            .fill_width()
            .min_width(0.0)
            .min_height(14.0)
            .font_size(
                12.0,
                "12 px chrome text that keeps its own line height and weight",
            )
            .text_color(sigil::prelude::Token::Gray11),
        );
    } else if let Some(comment) = note.and_then(|note| note.comment) {
        card = card.child(
            sigil::prelude::text(format!("note: {comment}"))
                .wrap()
                .font_size(
                    12.0,
                    "12 px chrome text that keeps its own line height and weight",
                )
                .text_color(sigil::prelude::Token::Info11)
                .fill_width()
                .min_width(0.0)
                .min_height(14.0)
                .padding_x(Space::Sm),
        );
    }
    card.child(crate::chat::entry::patch_view(
        app,
        hunk.body().to_string(),
        app.model.get().prefs.diff_layout,
        fit,
    ))
}
