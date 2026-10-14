mod chrome;
mod composer;
pub(super) mod entry;
mod footer;
pub(super) mod form;
pub(super) mod panels;
mod strip;
mod surface;
pub(super) mod todos;

pub(super) use super::app::App;
pub(super) use super::canvas;
pub(super) use super::model::{Agent, Call, Entry, Model};
pub(super) use super::pane::Pane;
pub(super) use super::panes;
pub(super) use super::speed::Speed;
pub(super) use super::suggest;
pub(super) use composer::busy_status;
pub(in crate::tui::sigil_view) use footer::Target;
pub(super) use sigil::app::cell_width;
#[cfg(feature = "gui")]
pub(super) use surface::transcript;
pub(super) use surface::{inline_layer, layer};

const ACTIVITY_SHARE: sigil::layout::Dimension = sigil::layout::Dimension::Percent(0.4);
