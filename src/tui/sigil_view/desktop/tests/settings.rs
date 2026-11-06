use sigil::events::{Event, ScrollDelta, ScrollPhase};
use sigil::geometry::Point;

use super::super::settings::{Page, open};
use super::harness::{Step, drive, steps};
use super::{VIEW, in_window};
use crate::modules::ModuleId;
use crate::tui::Cmd;

fn down() -> Event {
    Event::scroll(
        Point::new(VIEW.0 * 0.6, VIEW.1 / 2.0),
        ScrollDelta::Pixels(Point::new(0.0, -10_000.0)),
        ScrollPhase::Moved,
    )
}

#[test]
fn scrolling_a_settings_page_reveals_its_last_rows() {
    if !in_window("settings::scrolling_a_settings_page_reveals_its_last_rows") {
        return;
    }
    let seen = drive(|app| open(app, Page::Shortcuts), vec![down()], |_| ());
    assert!(seen.shows(|text| text == "Custom keys"), "scroll must reveal Custom keys: {}", seen.text());
}

#[test]
fn the_browser_switch_sends_its_typed_module() {
    if !in_window("settings::the_browser_switch_sends_its_typed_module") {
        return;
    }
    let seen = steps(|app| open(app, Page::Tools), vec![Step::Click("Browser control")], |_| ());
    assert!(
        seen.sent.iter().any(|cmd| matches!(cmd, Cmd::SetModule(ModuleId::Browser, true))),
        "the browser switch must turn on the browser module"
    );
}

#[test]
fn the_last_settings_page_shows_its_last_row() {
    if !in_window("settings::the_last_settings_page_shows_its_last_row") {
        return;
    }
    let seen = drive(|app| open(app, Page::Tools), vec![down()], |_| ());
    assert!(seen.shows(|text| text == "Browser app"), "{}", seen.text());
}
