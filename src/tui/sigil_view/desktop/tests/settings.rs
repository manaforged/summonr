use sigil::events::{
    ModifiersState, ScrollDelta, ScrollEvent, ScrollEventInit, ScrollPhase, ShellEvent,
};
use sigil::geometry::Point;
use sigil::prelude::Key;

use super::harness::{drive, named};
use super::{VIEW, in_window};
use crate::modules::ModuleId;
use crate::tui::Cmd;

#[test]
fn scrolling_settings_reveals_rows_without_changing_selection() {
    if !in_window("settings::scrolling_settings_reveals_rows_without_changing_selection") {
        return;
    }
    let seen = drive(
        |app| app.toggle_options(),
        vec![ShellEvent::Scroll(ScrollEvent::new(ScrollEventInit {
            position: Point::new(VIEW.0 / 2.0, VIEW.1 / 2.0),
            delta: ScrollDelta::Pixels(Point::new(0.0, -10_000.0)),
            modifiers: ModifiersState::empty(),
            phase: ScrollPhase::Moved,
        }))],
        |app| app.settings.cursor.peek(),
    );
    assert!(
        seen.shows(|text| text == "hotkeys"),
        "scroll must reveal hotkeys: {}",
        seen.text()
    );
    assert_eq!(seen.read, 0, "scroll must keep the selected setting");
}

#[test]
fn the_selected_module_row_sends_its_typed_switch() {
    if !in_window("settings::the_selected_module_row_sends_its_typed_switch") {
        return;
    }
    let seen = drive(
        |app| {
            app.toggle_options();
            let at = crate::tui::sigil_view::settings::setting_rows(app)
                .iter()
                .position(|row| row.label == "browser tools")
                .expect("browser module row");
            app.settings.select(at);
        },
        vec![named(Key::Enter, ModifiersState::empty())],
        |_| (),
    );
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::SetModule(ModuleId::Browser, true))),
        "the browser row must turn on the browser module"
    );
}
