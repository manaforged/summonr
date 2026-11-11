mod chrome;
mod composer;
pub(super) mod entry;
mod footer;
pub(super) mod form;
pub(super) mod panels;
pub(crate) mod strip;
pub(crate) mod surface;
pub(super) mod todos;

pub(super) use super::app::App;
pub(super) use super::canvas;
pub(super) use super::model::{Agent, Call, Entry, Model};
pub(super) use super::panes;
pub(super) use super::speed::Speed;
pub(super) use super::suggest;
pub(crate) use footer::Target;
pub(super) use sigil::app::cell_width;
pub(super) use surface::{inline_layer, layer};

const ACTIVITY_SHARE: sigil::prelude::Length = sigil::prelude::Length::Percent(0.4);
