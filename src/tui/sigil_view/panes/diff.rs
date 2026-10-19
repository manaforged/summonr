use sigil::layout::Dimension;
use sigil::prelude::Paint;
use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Tone,
    WidgetLayoutExt, button, chip, container, list_row, node, spacer, text_edit,
};

use std::collections::BTreeSet;

use super::App;
use super::body::{Empty, empty, header};
use crate::tui::sigil_view::canvas;

const ACCEPT: &str = "Accept";
const REVERT: &str = "Revert";
const NOTE: &str = "Note";
const UNDO: &str = "Undo";
const PENDING: &str = "pending";
const REVERT_FILE: &str = "Revert file";
const SCOPE_ALL: &str = "Showing: all changes vs base · show last turn";
const SESSION_OFF: &str = "This session only";
const SESSION_ON: &str = "All uncommitted changes";
const SCOPE_RECENT: &str = "Showing: files the last turn edited · show all";
const NO_CHANGES: Empty = Empty {
    window: "No changes yet. Files the agent edits show here while the turn runs.",
    terminal: "no changes",
};
use crate::tui::sigil_view::ui::{Hints, Key};

pub(super) fn diff(app: App, mut column: NodeBuilder, cols: u16) -> NodeBuilder {
    let model = app.model.get();
    if let Some(path) = app.diff_view.peek() {
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
                sigil::kernel::text("loading…")
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface)
                    .font_size(12.0)
                    .text_color(sigil::prelude::Token::Gray11),
            );
        };
        let text = match result {
            Ok(text) => text,
            Err(error) => {
                return column.child(
                    sigil::kernel::text(error.clone())
                        .wrap()
                        .layout(LayoutStyle::block().fill_width().min_size(0.0, 14.0))
                        .font_size(12.0)
                        .text_color(sigil::prelude::Token::Error11),
                );
            }
        };
        let hunks = app.hunks();
        if text.trim().is_empty() {
            column = column.child(
                sigil::kernel::text("no changes left in this file")
                    .nowrap()
                    .font_size(12.0)
                    .text_color(sigil::prelude::Token::Gray11),
            );
            return undo_rows(&app, column, Some(&path), &BTreeSet::new());
        }
        if hunks.is_empty() {
            return column.child(crate::tui::sigil_view::chat::entry::patch_view(
                &app,
                text.clone(),
                app.model.get().prefs.diff_layout,
                crate::tui::sigil_view::chat::entry::Fit::of(cols),
            ));
        }
        let cursor = app.hunk_cursor.peek();
        let notes = format!(
            "{} · {}",
            crate::text::plural(hunks.len(), "hunk"),
            Hints::new()
                .key(Key::Fixed("a"), "accept")
                .key(Key::Fixed("r"), "revert")
                .key(Key::Fixed("n"), "note")
                .text(&model.prefs.hotkeys)
        );
        column = column.child(
            sigil::kernel::text(notes)
                .wrap()
                .layout(LayoutStyle::block().fill_width().min_size(0.0, 14.0))
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
        drop(model);
        for (index, hunk) in hunks.iter().enumerate() {
            column = column.child(hunk_view(
                &app,
                &path,
                index,
                hunk,
                (
                    index == cursor,
                    crate::tui::sigil_view::chat::entry::Fit::of(cols),
                ),
            ));
        }
        return undo_rows(&app, column, Some(&path), &BTreeSet::new());
    }
    column = column.child(header(&app, "Changes".to_string(), model.diff.summary()));
    drop(model);
    let window = canvas::is_window();
    if window {
        column = column.child(scope(&app));
    }
    let stats = app.diff_stats();
    let listed: BTreeSet<String> = stats.iter().map(|stat| stat.path.clone()).collect();
    if stats.is_empty() {
        column = column.child(empty(&NO_CHANGES));
        return undo_rows(&app, column, None, &listed);
    }
    for (index, stat) in stats.into_iter().enumerate() {
        let open = app.clone();
        let path = stat.path.clone();
        let menu = app.clone();
        column = column.child(
            list_row(
                index == app.diff_cursor.peek(),
                stat.path.clone(),
                format!("+{} −{}", stat.added, stat.removed),
            )
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Right) {
                    menu.context_row(index);
                } else {
                    open.diff_cursor.set(index);
                    open.diff_open(path.clone());
                }
                event.stop_propagation();
            }),
        );
        if window {
            column = column.child(file_actions(&app, &stat.path));
        }
    }
    undo_rows(&app, column, None, &listed)
}

fn actions() -> NodeBuilder {
    container().layout(
        LayoutStyle::flex_row()
            .fill_width()
            .gap(6.0)
            .align_items(AlignItems::Center),
    )
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

fn file_actions(app: &App, path: &str) -> NodeBuilder {
    let (accepted, reverted) = app.file_review(path);
    let revert = app.clone();
    let target = path.to_string();
    let mut row = actions();
    if accepted == 0 && reverted == 0 {
        row = row.child(chip(PENDING, Tone::Neutral));
    }
    if accepted > 0 {
        row = row.child(chip(format!("{accepted} accepted"), Tone::Success));
    }
    if reverted > 0 {
        row = row.child(chip(format!("{reverted} reverted"), Tone::Warn));
    }
    row.child(spacer()).child(
        button(REVERT_FILE)
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
                    sigil::kernel::text(label)
                        .nowrap()
                        .ellipsize()
                        .layout(
                            LayoutStyle::block()
                                .grow(1.0)
                                .shrink(1.0)
                                .min_size(0.0, 14.0),
                        )
                        .font_size(12.0)
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

fn scope(app: &App) -> NodeBuilder {
    let toggle = app.clone();
    let label = if app.diff_recent.peek() {
        SCOPE_RECENT
    } else {
        SCOPE_ALL
    };
    let session = app.clone();
    let whole = if app.diff_session.peek() {
        SESSION_ON
    } else {
        SESSION_OFF
    };
    actions()
        .child(
            button(label)
                .ghost()
                .activate(move || toggle.toggle_recent()),
        )
        .child(
            button(whole)
                .ghost()
                .activate(move || session.toggle_session()),
        )
}

fn hunk_view(
    app: &App,
    path: &str,
    index: usize,
    hunk: &crate::hunks::Hunk,
    (focused, fit): (bool, crate::tui::sigil_view::chat::entry::Fit),
) -> NodeBuilder {
    let note = app.hunk_note(path, &hunk.id);
    let (verdict, tone) = match note.as_ref().and_then(|note| note.verdict) {
        Some(crate::hunks::Verdict::Accepted) => ("accepted", Tone::Success),
        Some(crate::hunks::Verdict::Rejected) => ("reverted", Tone::Warn),
        None => ("pending", Tone::Neutral),
    };
    let editing = app
        .note_target
        .peek()
        .is_some_and(|(target, id)| target == path && id == hunk.id);
    let menu = app.clone();
    let mut card = container()
        .layout(LayoutStyle::flex_column().fill_width().gap(4.0))
        .child(
            list_row(focused, hunk.header.clone(), String::new())
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .height(24.0)
                        .padding_x(8.0)
                        .gap(6.0)
                        .align_items(AlignItems::Center),
                )
                .child(chip(verdict, tone))
                .child(spacer())
                .pointer_down(move |event| {
                    if event.button == Some(MouseButton::Right) {
                        menu.hunk_context(index);
                    } else {
                        menu.hunk_cursor.set(index);
                        menu.focus_pane(true);
                    }
                    event.stop_propagation();
                }),
        );
    if canvas::is_window() && !editing {
        card = card.child(hunk_actions(app, path, &hunk.id));
    }
    if editing {
        let commit = app.clone();
        let edit = app.clone();
        card = card.child(node(move || {
            text_edit(edit.note_text)
                .caret_position(edit.note_caret)
                .submit(move |text| commit.note_commit(text))
                .autofocus()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .height(Dimension::Auto)
                        .min_size(0.0, 20.0),
                )
                .font_family(sigil::prelude::FontFamily::Mono)
                .font_size(12.0)
                .bg(sigil::prelude::Token::Gray1)
                .rounded(sigil::prelude::Radius::Md)
        }));
        card = card.child(
            sigil::kernel::text(format!(
                "{} · sent with your next prompt",
                Hints::new()
                    .key(Key::Enter, "save")
                    .key(Key::Esc, "cancel")
                    .text(&app.model.peek().prefs.hotkeys)
            ))
            .wrap()
            .layout(LayoutStyle::block().fill_width().min_size(0.0, 14.0))
            .font_size(12.0)
            .text_color(sigil::prelude::Token::Gray11),
        );
    } else if let Some(comment) = note.and_then(|note| note.comment) {
        card = card.child(
            sigil::kernel::text(format!("note: {comment}"))
                .wrap()
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Info11)
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .min_size(0.0, 14.0)
                        .padding_x(8.0),
                ),
        );
    }
    card.child(crate::tui::sigil_view::chat::entry::patch_view(
        app,
        hunk.body().to_string(),
        app.model.get().prefs.diff_layout,
        fit,
    ))
}
