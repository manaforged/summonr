use sigil::prelude::Position;
use sigil::prelude::Widget;
use sigil::prelude::{LayoutStyle, NodeBuilder, container};

use super::view::shell;
use crate::tui::Cmd;

pub(super) use super::app::App;
pub(super) use super::chat;
#[cfg(test)]
pub(super) use super::pane::Pane;

pub(super) mod entry;
mod thread;

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
        .child(thread::main_col(app))
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

#[cfg(test)]
mod tests;
