mod dialog;
mod hints;
mod keys;
mod prompt;
mod rows;

pub(in crate::tui) use dialog::{Dialog, Size};
pub(in crate::tui) use hints::{Hints, Key};
pub(in crate::tui::sigil_view) use keys::{Bindings, KeyAction, alt, keyed, press};
pub(in crate::tui) use prompt::{Action, ActionKind, Prompt};
pub(in crate::tui) use rows::{group, heading, notice, pick, slotted};

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
