mod dialog;
mod hints;
mod keys;
mod prompt;
mod rows;
mod tabs;

pub(in crate::tui) use dialog::{Dialog, Size};
pub(in crate::tui) use hints::{Hints, Key};
pub(in crate::tui::sigil_view) use keys::{Bindings, KeyAction, alt, keyed, press};
pub(in crate::tui) use prompt::{Action, ActionKind, Prompt};
pub(in crate::tui) use rows::{
    blank, field, field_width, group, heading, notice, part, pick, slotted,
};
pub(in crate::tui) use tabs::Tab;
pub(in crate::tui::sigil_view) use tabs::strip as tab_strip;

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
