use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Widget, button, container, spacer,
};

use super::super::style::{S, Text, XS, text};
use super::super::style::{CONTROL, MARK};
use crate::tui::Cmd;
use crate::tui::sigil_view::app::App;

/// What undoing the last turn's edits sends to the worker.
const UNDO: &str = "/undo files";

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
        .child(spacer());
    if undo {
        let sender = app.clone();
        row = row.child(
            button("Undo")
                .ghost()
                .size(ControlSize::Compact)
                .activate(move || drop(sender.cmd.send(Cmd::Command(UNDO.to_string())))),
        );
    }
    Some(
        container()
            .layout(LayoutStyle::block().fill_width().padding_bottom(S))
            .child(row),
    )
}

fn count(files: usize) -> String {
    match files {
        1 => "1 file changed".to_string(),
        files => format!("{files} files changed"),
    }
}
