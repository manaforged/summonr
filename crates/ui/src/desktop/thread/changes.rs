use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Widget, button, container, spacer,
};

use super::super::style::{CONTROL, MARK};
use super::super::style::{S, Text, XS, text};
use crate::app::App;
use summonr_agent::hub::Cmd;

/// What undoing the last turn's edits sends to the worker.
const UNDO: &str = "/undo files";
/// How many changed files the card lists before it counts the rest.
const FILE_ROWS: usize = 5;

/// The bar above the composer that counts changed files and offers Undo.
pub(super) fn bar(app: &App) -> Option<NodeBuilder> {
    let model = app.model.get();
    let files = &model.diff.files;
    if files.is_empty() {
        return None;
    }
    let undo = !model.recent.is_empty() && model.busy_since.is_none();
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_left(S + (CONTROL - MARK) / 2.0)
                .padding_right(XS)
                .gap(S)
                .align_items(AlignItems::Center),
        )
        .child(text(Text::Label, count(files.len())).nowrap())
        .child(
            text(
                Text::Meta,
                format!("+{} −{}", model.diff.added(), model.diff.removed()),
            )
            .nowrap(),
        )
        .child(spacer());
    if undo {
        let sender = app.clone();
        row = row.child(
            button("Undo")
                .ghost()
                .size(ControlSize::Compact)
                .activate(move || {
                    drop(sender.cmd.send(Cmd::Command(UNDO.to_string())));
                    drop(sender.cmd.send(Cmd::Diff(sender.root())));
                }),
        );
    }
    let list = files.iter().take(FILE_ROWS).fold(
        container().layout(LayoutStyle::flex_column().fill_width().padding_left(S)),
        |list, stat| list.child(file_row(app, &stat.path, stat.added, stat.removed)),
    );
    let more = files.len().saturating_sub(FILE_ROWS);
    let list = if more > 0 {
        list.child(
            container()
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .padding_left(S + (CONTROL - MARK) / 2.0),
                )
                .child(text(Text::Meta, format!("{more} more"))),
        )
    } else {
        list
    };
    Some(
        container()
            .layout(LayoutStyle::flex_column().fill_width().padding_bottom(S))
            .child(row)
            .child(list),
    )
}

/// One changed file; picking it opens Changes.
fn file_row(app: &App, path: &str, added: u32, removed: u32) -> NodeBuilder {
    let open = app.clone();
    button(format!("{path}  +{added} −{removed}"))
        .ghost()
        .size(ControlSize::Compact)
        .activate(move || open.toggle_pane(crate::pane::Pane::Diff))
        .into()
}

fn count(files: usize) -> String {
    match files {
        1 => "1 file changed".to_string(),
        files => format!("{files} files changed"),
    }
}
