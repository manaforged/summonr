use sigil::prelude::{LayoutStyle, NodeBuilder, Paint, Position, Widget, container, signal};
use sigil::reactive::untrack;

use super::super::App;
use super::super::style::{DIVIDER, FOCUS_RING, RING};
use super::{COLUMNS, with};

/// The panes in a grid: one, two side by side, or two rows of two.
pub(crate) fn view() -> NodeBuilder {
    let count = with(|split| split.count).unwrap_or_else(|| signal(1));
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_height()
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            let n = count.get();
            let apps: Vec<App> =
                with(|split| split.panes.iter().map(|pane| pane.app.clone()).collect())
                    .unwrap_or_default();
            untrack(|| pane_rows(&apps, n))
        })
}

fn pane_rows(apps: &[App], n: usize) -> Vec<NodeBuilder> {
    if n <= 1 {
        return apps
            .first()
            .map(|app| super::super::terminal::pane(app.clone()))
            .into_iter()
            .collect();
    }
    let columns = COLUMNS;
    apps.chunks(columns)
        .enumerate()
        .map(|(row, chunk)| {
            chunk.iter().enumerate().fold(
                container().layout(
                    LayoutStyle::flex_row()
                        .grow(1.0)
                        .flex_basis(0.0)
                        .fill_width()
                        .min_size(0.0, 0.0),
                ),
                |line, (column, app)| line.child(cell(app, row * columns + column)),
            )
        })
        .collect()
}

fn cell(app: &App, at: usize) -> NodeBuilder {
    let focus = with(|split| split.focus).unwrap_or_else(|| signal(0));
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .flex_basis(0.0)
                .fill_height()
                .min_size(0.0, 0.0)
                .position(Position::Relative),
        )
        .border_w(1)
        .border_color(DIVIDER)
        .pointer_down(move |_| {
            if focus.peek() != at {
                focus.set(at);
            }
        })
        .child(super::super::terminal::pane(app.clone()))
        .children_reactive(move || {
            if focus.get() == at {
                ring()
            } else {
                Vec::new()
            }
        })
}

/// Four thin edges around the focused pane; edges only, so clicks reach the pane.
fn ring() -> Vec<NodeBuilder> {
    let edge = |layout: LayoutStyle| {
        container()
            .layout(layout.position(Position::Absolute))
            .bg(FOCUS_RING)
    };
    vec![
        edge(
            LayoutStyle::block()
                .inset_top(0.0)
                .inset_left(0.0)
                .fill_width()
                .height(RING),
        ),
        edge(
            LayoutStyle::block()
                .inset_bottom(0.0)
                .inset_left(0.0)
                .fill_width()
                .height(RING),
        ),
        edge(
            LayoutStyle::block()
                .inset_top(0.0)
                .inset_left(0.0)
                .fill_height()
                .width(RING),
        ),
        edge(
            LayoutStyle::block()
                .inset_top(0.0)
                .inset_right(0.0)
                .fill_height()
                .width(RING),
        ),
    ]
}
