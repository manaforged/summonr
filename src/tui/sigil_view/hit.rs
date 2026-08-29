use std::cell::RefCell;
use std::rc::Rc;

use sigil::prelude::*;
use sigil_tui::Host;

use super::app::App;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Setting(usize),
    Model(usize),
    Auth(usize),
    Role(usize),
    Saved(String),
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

    #[cfg(test)]
    pub(super) fn find(&self, target: &Target) -> Option<(usize, usize)> {
        self.0
            .borrow()
            .iter()
            .find(|hit| hit.target == *target)
            .map(|hit| (hit.from, hit.row))
    }
}

fn cell(event: &PointerEvent) -> (usize, usize) {
    let col = (event.position.x / Host::CELL).max(0.0) as usize;
    let row = (event.position.y / Host::CELL).max(0.0) as usize;
    (col, row)
}

pub(super) fn wire(app: &App, layer: NodeBuilder) -> NodeBuilder {
    let press = app.clone();
    let drag = app.clone();
    let release = app.clone();
    layer
        .pointer_down(move |event| {
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
            drag.drag_to(col, row);
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
