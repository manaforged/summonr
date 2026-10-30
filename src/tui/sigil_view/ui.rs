mod dialog;
mod hints;
mod keys;
mod list;
mod prompt;
mod rows;
mod tabs;

pub(in crate::tui::sigil_view) use dialog::dismissing;
pub(in crate::tui) use dialog::{Dialog, Size};
pub(in crate::tui) use hints::{FILTER_HINT, Hints, Key, alt_label};
pub(in crate::tui::sigil_view) use keys::{Bindings, KeyAction, alt, input_keys, keyed, press};
pub(in crate::tui) use list::{cycle, shift};
pub(in crate::tui) use prompt::{Action, ActionKind, Prompt};
pub(in crate::tui::sigil_view) use rows::filter_field;
pub(in crate::tui) use rows::{
    blank, browser_columns, browser_layout, browser_split, field, field_width, filter_column,
    group, heading, notice, overlay_list, part, pick, plain, slotted,
};
pub(in crate::tui) use sigil::widgets::ListState as List;
pub(in crate::tui) use tabs::Tab;
pub(in crate::tui::sigil_view) use tabs::strip as tab_strip;

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
