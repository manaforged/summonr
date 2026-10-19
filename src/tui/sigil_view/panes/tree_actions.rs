use std::path::Path;

use sigil::prelude::{
    ControlSize, LayoutBuilderExt, LayoutStyle, NodeBuilder, Tone, button, container,
};

use super::super::app::App;
use super::super::canvas;

const MERGE: &str = "Merge";
const DISCARD: &str = "Discard";
const CONFIRM_DISCARD: &str = "Confirm discard";

pub(super) fn buttons(app: &App, path: &Path) -> NodeBuilder {
    let row = container().layout(LayoutStyle::flex_row().shrink(0.0).gap(4.0));
    if !canvas::is_window() {
        return row;
    }
    let (merge, discard) = (app.clone(), app.clone());
    let (merged, discarded) = (path.to_path_buf(), path.to_path_buf());
    let label = if app.worktree_discard.peek().as_deref() == Some(path) {
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
