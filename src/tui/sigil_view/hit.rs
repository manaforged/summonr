use std::cell::RefCell;
use std::rc::Rc;

use super::app::App;
use super::canvas::cell as metrics;
use sigil::prelude::{MouseButton, NodeBuilder, NodeEventsExt, PointerEvent};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Setting(usize),
    Hotkey(usize),
    Model(usize),
    Session(usize),
    Search(usize),
    Auth(usize),
    Role(usize),
    Saved(String),
    File(usize),
    Close,
    Context,
    Cache,
    AgentsClose,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hit {
    pub(super) row: usize,
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) target: Target,
}

#[derive(Clone, Default)]
pub(super) struct Hits(Rc<RefCell<Vec<Hit>>>);

impl Hits {
    pub(super) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    pub(super) fn add(&self, row: usize, from: usize, to: usize, target: Target) {
        self.0.borrow_mut().push(Hit {
            row,
            from,
            to,
            target,
        });
    }

    pub(super) fn at(&self, col: usize, row: usize) -> Option<Target> {
        self.0
            .borrow()
            .iter()
            .find(|hit| hit.row == row && (hit.from..hit.to).contains(&col))
            .map(|hit| hit.target.clone())
    }
}

pub(super) fn window(app: &App, selected: usize, len: usize, visible: usize) -> usize {
    let visible = visible.max(1);
    let top = app.scroll.peek();
    let start = if selected < top {
        selected
    } else if selected >= top + visible {
        selected + 1 - visible
    } else {
        top
    }
    .min(len.saturating_sub(visible));
    if start != top {
        app.scroll.set(start);
    }
    start
}

fn cell(event: &PointerEvent) -> (usize, usize) {
    let cell = metrics();
    let col = (event.position.x / cell.w).max(0.0) as usize;
    let row = (event.position.y / cell.h).max(0.0) as usize;
    (col, row)
}

pub(super) fn wire(app: &App, layer: NodeBuilder) -> NodeBuilder {
    let press = app.clone();
    let drag = app.clone();
    let release = app.clone();
    layer
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Right) {
                let (col, row) = cell(event);
                if press.context_at(col, row) {
                    event.stop_propagation();
                }
                return;
            }
            if event.button != Some(MouseButton::Left) {
                return;
            }
            let (col, row) = cell(event);
            event.stop_propagation();
            if press.press(col, row) {
                event.capture_pointer();
            }
        })
        .pointer_move(move |event| {
            let (col, row) = cell(event);
            drag.hover(col, row);
        })
        .pointer_up(move |event| {
            if event.button != Some(MouseButton::Left) {
                return;
            }
            let (col, row) = cell(event);
            release.release(col, row);
            event.stop_propagation();
        })
}
