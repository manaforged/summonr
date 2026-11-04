use sigil::prelude::Position;
use sigil::prelude::Widget;
use sigil::prelude::{LayoutStyle, NodeBuilder, container};

use super::view::shell;
use crate::tui::Cmd;

pub(super) use super::app::App;
pub(super) use super::chat;
pub(super) use super::pane::Pane;

pub(super) mod entry;
mod thread;
pub(super) mod welcome;

pub(super) fn root(app: App) -> NodeBuilder {
    drop(app.cmd.send(Cmd::Diff(None)));
    let paint = app.clone();
    shell(&app).children_reactive(move || {
        vec![window(paint.clone()).theme(super::theme::gui(paint.appearance.get()))]
    })
}

fn window(app: App) -> NodeBuilder {
    let overlays = app.clone();
    container()
        .layout(LayoutStyle::flex_column().fill())
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill()
                        .grow(1.0)
                        .min_size(0.0, 0.0),
                )
                .children_reactive(move || vec![content(app.clone())]),
        )
        .children_reactive(move || {
            let _viewport = overlays.viewport.get();
            overlays
                .overlay()
                .and_then(|overlay| overlay.layer(overlays.clone()))
                .map(|layer| {
                    container()
                        .layout(
                            LayoutStyle::flex_column()
                                .position(Position::Absolute)
                                .fill(),
                        )
                        .child(layer)
                })
        })
}

fn content(app: App) -> NodeBuilder {
    let layout = app.layout.get();
    let (cols, _) = app.viewport.get();
    let (_, side_cols) = layout.split(cols);
    let row = container()
        .layout(LayoutStyle::flex_row().fill().min_size(0.0, 0.0))
        .child(sigil::reactive::untrack(|| thread::main_col(app.clone())));
    match layout.secondary {
        Some(pane) if side_cols > 0 => row.child(
            container().layout(LayoutStyle::flex_column().fill_height().shrink(0.0))
                .children_reactive(move || vec![super::panes::body(&app, pane, side_cols)]),
        ),
        Some(_) | None => row,
    }
}

#[cfg(test)]
mod tests;
