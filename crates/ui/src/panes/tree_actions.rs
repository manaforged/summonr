use std::path::Path;

use sigil::prelude::Space;
use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Token, Tone, button, container, spacer, text,
};

use super::super::app::{App, armed};
use super::super::canvas;
use sigil::prelude::{Paint, Widget};
use summonr_agent::hub::Cmd;

const MERGE: &str = "Merge";
const DISCARD: &str = "Discard";
const CONFIRM_DISCARD: &str = "Confirm discard";
const UNDO: &str = "Undo";
const UNDO_FILES: &str = "/undo files";

/// The Changes panel's top row: this agent's branch, Undo after an edit, and Merge and Discard in a worktree.
pub(super) fn review(app: &App) -> Option<NodeBuilder> {
    if !canvas::is_window() {
        return None;
    }
    let model = app.model.get();
    let workspace = app.workspace();
    let tree = model.trees.iter().find(|seen| seen.tree.path == workspace);
    let label = match tree {
        Some(seen) if seen.tree.base.is_empty() => seen.tree.branch.clone(),
        Some(seen) => format!("{} → {}", seen.tree.branch, seen.tree.base),
        None => app.git_branch()?,
    };
    let row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .shrink(0.0)
                .gap(Space::Xs)
                .align_items(AlignItems::Center),
        )
        .child(
            text(label)
                .nowrap()
                .font_size(13.0, "13 px between caption and body for dense code rows")
                .text_color(Token::OnSurfaceMuted),
        )
        .child(spacer());
    let row = if !model.recent.is_empty() && model.busy_since.is_none() {
        let sender = app.clone();
        row.child(
            button(UNDO)
                .ghost()
                .size(ControlSize::Compact)
                .activate(move || {
                    drop(sender.cmd.send(Cmd::Command(UNDO_FILES.to_string())));
                    drop(sender.cmd.send(Cmd::Diff(sender.root())));
                }),
        )
    } else {
        row
    };
    Some(match tree.filter(|_| model.busy_since.is_none()) {
        Some(seen) => row.child(buttons(app, &seen.tree.path)),
        None => row,
    })
}

pub(super) fn buttons(app: &App, path: &Path) -> NodeBuilder {
    let row = container().layout(LayoutStyle::flex_row().shrink(0.0).gap(Space::Xs));
    if !canvas::is_window() {
        return row;
    }
    let (merge, discard) = (app.clone(), app.clone());
    let (merged, discarded) = (path.to_path_buf(), path.to_path_buf());
    let label = if armed(&app.worktree_discard, &discarded) {
        CONFIRM_DISCARD
    } else {
        DISCARD
    };
    row.child(
        button(MERGE)
            .ghost()
            .size(ControlSize::Compact)
            .activate(move || merge.tree_merge(merged.clone())),
    )
    .child(
        button(label)
            .ghost()
            .tone(Tone::Warn)
            .size(ControlSize::Compact)
            .activate(move || discard.tree_discard_armed(discarded.clone())),
    )
}
