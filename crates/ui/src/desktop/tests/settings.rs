use sigil::events::{Event, ScrollDelta, ScrollPhase};
use sigil::geometry::Point;

use super::super::settings::{Page, open};
use super::harness::{Step, drive, steps};
use super::{VIEW, in_window};
use summonr_agent::hub::Cmd;
use summonr_agent::modules::ModuleId;

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
    assert!(
        seen.shows(|text| text == "Custom keys"),
        "scroll must reveal Custom keys: {}",
        seen.text()
    );
}

#[test]
fn the_browser_switch_sends_its_typed_module() {
    if !in_window("settings::the_browser_switch_sends_its_typed_module") {
        return;
    }
    let seen = steps(
        |app| open(app, Page::Tools),
        vec![Step::Click("Browser control")],
        |_| (),
    );
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::SetModule(ModuleId::Browser, true))),
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

#[test]
fn the_mcp_form_asks_for_environment_or_headers() {
    if !in_window("settings::the_mcp_form_asks_for_environment_or_headers") {
        return;
    }
    let seen = steps(
        |app| open(app, Page::Mcp),
        vec![Step::Click("Add server")],
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "Environment or headers"),
        "{}",
        seen.text()
    );
}

#[test]
fn the_tools_page_says_what_a_switch_reaches() {
    if !in_window("settings::the_tools_page_says_what_a_switch_reaches") {
        return;
    }
    let seen = drive(|app| open(app, Page::Tools), Vec::new(), |_| ());
    assert!(
        seen.shows(|text| text == "Agents already working keep their tools"),
        "{}",
        seen.text()
    );
}

#[test]
fn the_shortcuts_page_lists_new_terminal() {
    if !in_window("settings::the_shortcuts_page_lists_new_terminal") {
        return;
    }
    let seen = drive(|app| open(app, Page::Shortcuts), Vec::new(), |_| ());
    assert!(seen.shows(|text| text == "New terminal"), "{}", seen.text());
}

#[test]
fn the_plugins_page_groups_skills_by_folder_and_picks_the_set_for_new_agents() {
    if !in_window(
        "settings::the_plugins_page_groups_skills_by_folder_and_picks_the_set_for_new_agents",
    ) {
        return;
    }
    let home = summonr_core::dirs::summonr_home().unwrap();
    let skill = home.join("skills").join("review").join("rust");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "# Rust review\n").unwrap();
    let config = home.join("config.toml");
    std::fs::write(&config, "[skills.sets.lean]\ninclude = [\"review\"]\n").unwrap();
    let seen = steps(
        |app| open(app, Page::Plugins),
        vec![Step::Click("lean")],
        |_| (),
    );
    for shown in [
        "Skills · review",
        "rust",
        "Skill set for new agents",
        "All skills",
        "none",
    ] {
        assert!(seen.shows(|text| text == shown), "{shown}: {}", seen.text());
    }
    let written = std::fs::read_to_string(&config).unwrap();
    assert!(written.contains("default = \"lean\""), "{written}");
}

#[test]
fn dragging_the_spacing_slider_to_its_end_saves_the_widest_gap() {
    if !in_window("settings::dragging_the_spacing_slider_to_its_end_saves_the_widest_gap") {
        return;
    }
    let seen = steps(
        |app| open(app, Page::Appearance),
        vec![Step::DragControl("Paragraph spacing", (600.0, 0.0))],
        |app| app.model.peek().prefs.spacing,
    );
    assert_eq!(
        seen.read,
        *summonr_agent::prefs::SPACING.end(),
        "{}",
        seen.text()
    );
}
