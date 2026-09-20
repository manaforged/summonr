use super::canvas::ellipsize;

mod chrome;
mod composer;
pub(super) mod entry;
mod footer;
mod panels;
mod surface;
pub(super) mod todos;
#[cfg(test)]
pub(crate) mod transcript;

pub(super) use super::app::App;
pub(super) use super::canvas;
#[cfg(test)]
pub(super) use super::input::Input;
#[cfg(test)]
pub(super) use super::model;
pub(super) use super::model::{Entry, Model};
pub(super) use super::pane::Pane;
pub(super) use super::panes;
#[cfg(test)]
pub(super) use super::patch;
pub(super) use super::prelude;
pub(super) use super::speed::Speed;
pub(super) use super::suggest;
pub(super) use composer::{busy_status, composer_hint, draft_view, margin};
pub(in crate::tui::sigil_view) use footer::Target;
pub(super) use sigil_tui::cell_width;
pub(super) use surface::layer;

pub(super) fn secret_row(prompt: &str, typed: usize, width: usize) -> String {
    const KEYS: &str = "   enter / esc";
    let dots = "•".repeat(typed.min(24));
    let label = if prompt.is_empty() { "secret" } else { prompt };
    let room = width.saturating_sub(cell_width(&dots) + cell_width(KEYS) + 12);
    format!("password {}  {dots}{KEYS}", ellipsize(label, room))
}
