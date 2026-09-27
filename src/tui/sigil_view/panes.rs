pub(super) use super::app::{App, FileRow};
pub(super) use super::pane::Pane;
use sigil::prelude::NodeBuilder;

pub(in crate::tui::sigil_view) mod body;
mod diff;
mod jobs;
mod peers;
mod todos;

pub(super) mod ledger;

pub(super) fn body(app: &App, pane: Pane, cols: u16) -> NodeBuilder {
    body::pane_body(app.clone(), pane, f32::from(cols) * super::canvas::cell().w)
}
