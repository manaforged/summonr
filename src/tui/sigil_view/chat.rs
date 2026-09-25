use crate::text::ellipsize;

mod chrome;
mod composer;
pub(super) mod entry;
mod footer;
mod panels;
mod surface;
pub(super) mod todos;

pub(super) use super::app::App;
pub(super) use super::canvas;
pub(super) use super::model::{Entry, Model};
pub(super) use super::pane::Pane;
pub(super) use super::panes;
pub(super) use super::prelude;
pub(super) use super::speed::Speed;
pub(super) use super::suggest;
pub(super) use composer::{busy_status, composer_hint, draft_view};
pub(in crate::tui::sigil_view) use footer::Target;
pub(super) use sigil::app::cell_width;
pub(super) use surface::layer;
#[cfg(feature = "gui")]
pub(super) use surface::transcript;

pub(super) fn secret_row(prompt: &str, typed: usize, width: usize) -> String {
    const KEYS: &str = "   enter / esc";
    let dots = "•".repeat(typed.min(24));
    let label = if prompt.is_empty() { "secret" } else { prompt };
    let room = width.saturating_sub(cell_width(&dots) + cell_width(KEYS) + 12);
    format!("password {}  {dots}{KEYS}", ellipsize(label, room))
}
