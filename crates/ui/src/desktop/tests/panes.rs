use sigil::events::{Key, LogicalKey, ModifiersState};
use sigil::prelude::Role;
use summonr_agent::prefs::{Axis, Shape, WHOLE};

use super::harness::{Step, named, press, steps};
use super::in_window;
use crate::desktop::split;

const CMD: ModifiersState = ModifiersState::META;
const CMD_ALT: ModifiersState = ModifiersState::META.union(ModifiersState::ALT);
const CMD_SHIFT: ModifiersState = ModifiersState::META.union(ModifiersState::SHIFT);

fn cmd(key: &str, modifiers: ModifiersState) -> Step {
    Step::Key(press(LogicalKey::character(key), modifiers, Some(key)))
}

fn layout(shape: &Shape) -> String {
    match shape {
        Shape::Pane(at) => at.to_string(),
        Shape::Split { axis, parts } => {
            let inner: Vec<String> = parts.iter().map(|part| layout(&part.shape)).collect();
            let sep = if *axis == Axis::Right { " | " } else { " / " };
            format!("({})", inner.join(sep))
        }
    }
}

fn strips<T>(seen: &super::harness::Seen<T>) -> usize {
    seen.named(Role::TextInput, "What should this agent do?")
        .len()
}

fn notice(app: &super::super::App) -> String {
    app.notice
        .peek()
        .map(|notice| notice.text)
        .unwrap_or_default()
}

#[test]
fn panes_split_right_and_down_until_they_run_out_of_room() {
    if !in_window("panes::panes_split_right_and_down_until_they_run_out_of_room") {
        return;
    }
    let seen = steps(
        |_| {},
        vec![cmd("d", CMD), cmd("d", CMD_ALT)],
        |app| (layout(&split::shape()), split::tracked().1, notice(app)),
    );
    assert_eq!(seen.read, ("(0 | (1 / 2))".to_owned(), 2, String::new()));
    let seen = steps(
        |_| {},
        vec![cmd("d", CMD), cmd("d", CMD_ALT), cmd("d", CMD)],
        |app| (layout(&split::shape()), notice(app)),
    );
    assert_eq!(
        seen.read.0, "(0 | (1 / 2))",
        "a pane too narrow to halve is not split"
    );
    assert!(
        seen.read.1.starts_with("No room to split"),
        "{:?}",
        seen.read.1
    );
}

#[test]
fn arrows_move_focus_and_swap_panes_and_zoom_shows_one() {
    if !in_window("panes::arrows_move_focus_and_swap_panes_and_zoom_shows_one") {
        return;
    }
    let seen = steps(
        |_| {},
        vec![
            cmd("d", CMD),
            cmd("d", CMD_ALT),
            Step::Key(named(Key::ArrowUp, CMD_ALT)),
            Step::Key(named(Key::ArrowLeft, CMD_ALT)),
        ],
        |_| split::tracked().1,
    );
    assert_eq!(
        seen.read, 0,
        "up from the bottom right, then left, lands on the first pane"
    );
    let seen = steps(
        |_| {},
        vec![
            cmd("d", CMD),
            cmd("d", CMD_ALT),
            Step::Key(named(Key::ArrowLeft, CMD_ALT | ModifiersState::SHIFT)),
        ],
        |_| (layout(&split::shape()), split::tracked().1),
    );
    assert_eq!(seen.read, ("(2 | (1 / 0))".to_owned(), 2));
    let split = steps(|_| {}, vec![cmd("d", CMD)], |_| ());
    let zoomed = steps(
        |_| {},
        vec![cmd("d", CMD), Step::Key(named(Key::Enter, CMD_SHIFT))],
        |_| split::apps().len(),
    );
    assert_eq!(strips(&split), 2, "{:?}", split.controls);
    assert_eq!(
        (strips(&zoomed), zoomed.read),
        (1, 2),
        "zoom hides the other pane without closing it"
    );
}

#[test]
fn closing_the_middle_pane_gives_its_space_to_its_neighbour() {
    if !in_window("panes::closing_the_middle_pane_gives_its_space_to_its_neighbour") {
        return;
    }
    let seen = steps(
        |_| {},
        vec![cmd("d", CMD), cmd("d", CMD_ALT), cmd("w", CMD)],
        |_| layout(&split::shape()),
    );
    assert_eq!(seen.read, "(0 | 1)");
}

#[test]
fn dragging_a_border_saves_the_new_shares() {
    if !in_window("panes::dragging_a_border_saves_the_new_shares") {
        return;
    }
    let seen = steps(
        |_| {},
        vec![
            cmd("d", CMD),
            Step::DragControl("Resize panes", (200.0, 0.0)),
        ],
        |_| split::shape(),
    );
    let Shape::Split { parts, .. } = &seen.read else {
        panic!("expected two panes, got {:?}", seen.read);
    };
    let (left, right) = (parts[0].share, parts[1].share);
    assert!(left > right, "the left pane grew: {left} against {right}");
    assert_eq!(left + right, WHOLE, "shares cover the split");
}

#[cfg(feature = "menu")]
#[test]
fn the_menu_bar_opens_the_window_sidebar_and_settings_from_a_split_pane() {
    if !in_window("panes::the_menu_bar_opens_the_window_sidebar_and_settings_from_a_split_pane") {
        return;
    }
    let seen = steps(
        |app| app.window.sidebar.set(true),
        vec![
            cmd("d", CMD),
            Step::Wait(Box::new(|app| {
                super::super::menubar::run(app, "Sidebar");
                super::super::menubar::run(app, "Settings…");
            })),
        ],
        |app| (split::tracked().1, app.window.sidebar.peek(), app.settings_open.peek()),
    );
    assert_eq!(seen.read, (1, false, true));
}
