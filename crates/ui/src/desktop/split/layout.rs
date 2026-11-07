use sigil::prelude::{
    LayoutStyle, NodeBuilder, Paint, Position, Signal, Widget, container, effect, signal,
};
use sigil::reactive::untrack;
use sigil::widgets::Orientation;
use summonr_agent::prefs::{Axis, Part, Shape};

use super::super::App;
use super::super::style::{DIVIDER, FOCUS_RING, PANE_HEIGHT, PANE_WIDTH, RING};
use super::{Bounds, with};

/// What a pane is drawn from: its agent's view and where it reports its bounds.
type Drawn = (App, Signal<Bounds>);

/// The panes as their split tree, or the zoomed pane alone.
pub(crate) fn view() -> NodeBuilder {
    let revision = with(|split| split.revision).unwrap_or_else(|| signal(0));
    container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_height()
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            revision.get();
            let Some((panes, shape)) = with(|split| {
                let panes: Vec<Drawn> = split
                    .panes
                    .iter()
                    .map(|pane| (pane.app.clone(), pane.bounds))
                    .collect();
                (
                    panes,
                    split.zoom.map_or_else(|| split.shape.clone(), Shape::Pane),
                )
            }) else {
                return Vec::new();
            };
            let framed = matches!(shape, Shape::Split { .. });
            untrack(|| vec![node(&panes, &shape, Vec::new(), framed)])
        })
}

fn node(panes: &[Drawn], shape: &Shape, path: Vec<usize>, framed: bool) -> NodeBuilder {
    match shape {
        Shape::Pane(at) => panes
            .get(*at)
            .map_or_else(container, |(app, bounds)| cell(app, *at, *bounds, framed)),
        Shape::Split { axis, parts } => split(panes, *axis, parts, path),
    }
}

fn split(panes: &[Drawn], axis: Axis, parts: &[Part], path: Vec<usize>) -> NodeBuilder {
    let sizes = signal(Vec::new());
    let at = path.clone();
    effect(move || {
        let next = sizes.get();
        with(|split| split.shape.set_sizes(&at, &next));
    });
    let (orientation, least) = match axis {
        Axis::Right => (Orientation::Horizontal, PANE_WIDTH),
        Axis::Down => (Orientation::Vertical, PANE_HEIGHT),
    };
    let last = parts.len().saturating_sub(1);
    let mut row = sigil::widgets::splitter(orientation);
    for (i, part) in parts.iter().enumerate() {
        let mut inner = path.clone();
        inner.push(i);
        let pane = row.pane(node(panes, &part.shape, inner, true)).min(least);
        #[allow(
            clippy::cast_precision_loss,
            reason = "a pane size is a small pixel count"
        )]
        let size = part.size as f32;
        row = if i == last || part.size == 0 {
            pane.grow(1.0)
        } else {
            pane.initial(size)
        }
        .commit();
    }
    row.sizes_bind(sizes).grow(1.0).into()
}

fn cell(app: &App, at: usize, bounds: Signal<Bounds>, framed: bool) -> NodeBuilder {
    let focus = with(|split| split.focus).unwrap_or_else(|| signal(0));
    let cell = container()
        .layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .flex_basis(0.0)
                .fill_height()
                .min_size(0.0, 0.0)
                .position(Position::Relative),
        )
        .bounds_reactive(bounds);
    if !framed {
        return cell.child(super::super::terminal::pane(app.clone()));
    }
    cell.border_w(1)
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
