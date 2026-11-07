use summonr_agent::prefs::{Axis, Shape};

use super::super::App;
use super::super::style::{PANE_HEIGHT, PANE_WIDTH};
use super::{Bounds, changed, focus, with};
use crate::view::keys::bindings::Dir;

/// How far two panes may overlap at a shared edge and still count as touching.
const EDGE: f32 = 2.0;

/// Where each pane sits; tracked.
pub(in crate::desktop) fn shape() -> Shape {
    let Some(revision) = with(|split| split.revision) else {
        return Shape::default();
    };
    revision.with(|_| ());
    with(|split| split.shape.clone()).unwrap_or_default()
}

/// Puts the panes where `saved` says, when it holds exactly the open panes.
pub(in crate::desktop) fn arrange(saved: Option<Shape>) {
    with(|split| {
        let count = split.panes.len();
        let shape = saved
            .filter(|shape| {
                let mut panes = shape.panes();
                panes.sort_unstable();
                panes == (0..count).collect::<Vec<_>>()
            })
            .unwrap_or_else(|| Shape::row(count));
        split.shape = shape;
    });
    changed();
}

pub(super) const fn along(bounds: Bounds, axis: Axis) -> f32 {
    match axis {
        Axis::Right => bounds.2,
        Axis::Down => bounds.3,
    }
}

const fn least(axis: Axis) -> f32 {
    match axis {
        Axis::Right => PANE_WIDTH,
        Axis::Down => PANE_HEIGHT,
    }
}

/// Whether a pane `extent` pixels long has room for two; an unmeasured pane always does.
pub(super) fn room(extent: f32, axis: Axis) -> bool {
    extent <= 0.0 || extent / 2.0 >= least(axis)
}

pub(super) fn refuse(main: &App, axis: Axis) {
    let (size, side) = match axis {
        Axis::Right => (PANE_WIDTH, "wide"),
        Axis::Down => (PANE_HEIGHT, "tall"),
    };
    main.notice.set(Some(crate::app::Notice {
        text: format!("No room to split: each pane needs {size} px {side}"),
        kind: crate::app::NoticeKind::Failed,
    }));
}

/// Moves focus to the nearest pane in `dir`.
pub(super) fn go(dir: Dir) {
    if unzoom() {
        return;
    }
    if let Some((_, to)) = neighbour(dir) {
        focus(to);
    }
}

/// Swaps the focused pane with the nearest pane in `dir`; focus stays with the moved pane.
pub(super) fn swap(dir: Dir) {
    if unzoom() {
        return;
    }
    if let Some((from, to)) = neighbour(dir) {
        with(|split| split.shape.swap(from, to));
        changed();
    }
}

/// Shows only the focused pane, or every pane again.
pub(super) fn zoom() {
    let toggled = with(|split| {
        let at = split.focus.peek();
        split.zoom = match split.zoom {
            Some(_) => None,
            None => (split.panes.len() > 1).then_some(at),
        };
    });
    if toggled.is_some() {
        changed();
    }
}

fn unzoom() -> bool {
    let zoomed = with(|split| split.zoom.take().is_some()).unwrap_or(false);
    if zoomed {
        changed();
    }
    zoomed
}

fn neighbour(dir: Dir) -> Option<(usize, usize)> {
    with(|split| {
        let at = split.focus.peek();
        let from = split.panes.get(at)?.bounds.peek();
        let others = split
            .panes
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != at)
            .map(|(other, pane)| (other, pane.bounds.peek()));
        nearest(from, dir, others).map(|to| (at, to))
    })
    .flatten()
}

/// The pane past `from`'s edge in `dir` that is closest, then shares the most of that edge.
fn nearest(from: Bounds, dir: Dir, others: impl Iterator<Item = (usize, Bounds)>) -> Option<usize> {
    others
        .filter_map(|(at, bounds)| reach(from, bounds, dir).map(|(gap, shared)| (gap, shared, at)))
        .min_by(|a, b| a.0.total_cmp(&b.0).then(b.1.total_cmp(&a.1)))
        .map(|(_, _, at)| at)
}

fn reach(from: Bounds, to: Bounds, dir: Dir) -> Option<(f32, f32)> {
    let ((fx, fy, fw, fh), (tx, ty, tw, th)) = (from, to);
    let (gap, shared) = match dir {
        Dir::Right => (tx - (fx + fw), overlap(fy, fh, ty, th)),
        Dir::Left => (fx - (tx + tw), overlap(fy, fh, ty, th)),
        Dir::Down => (ty - (fy + fh), overlap(fx, fw, tx, tw)),
        Dir::Up => (fy - (ty + th), overlap(fx, fw, tx, tw)),
    };
    (gap >= -EDGE && shared > 0.0).then_some((gap, shared))
}

fn overlap(a: f32, a_len: f32, b: f32, b_len: f32) -> f32 {
    (a + a_len).min(b + b_len) - a.max(b)
}

#[cfg(test)]
#[path = "arrange_tests.rs"]
mod tests;
