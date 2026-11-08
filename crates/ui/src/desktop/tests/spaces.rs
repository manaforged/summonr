use sigil::events::{Key, LogicalKey, ModifiersState};
use sigil::prelude::Role;

use super::harness::{Step, named, press, steps, typed};
use super::in_window;
use super::sidebar::two_chats;

fn rows(controls: &[(Role, String, bool)]) -> Vec<&str> {
    controls
        .iter()
        .filter(|(role, _, _)| *role == Role::ListItem)
        .map(|(_, name, _)| name.as_str())
        .collect()
}

#[test]
fn an_agent_moved_into_a_new_workspace_lists_under_it_and_leaves_its_repository() {
    if !in_window(
        "spaces::an_agent_moved_into_a_new_workspace_lists_under_it_and_leaves_its_repository",
    ) {
        return;
    }
    let mut actions = vec![
        Step::Click("New workspace"),
        Step::Key(press(LogicalKey::character("a"), ModifiersState::META, Some("a"))),
    ];
    actions.extend(typed("Launch").into_iter().map(Step::Key));
    actions.extend([
        Step::Key(named(Key::Enter, ModifiersState::empty())),
        Step::RightClick("Fix the parser bug"),
        Step::Click("Move to Launch"),
    ]);
    let seen = steps(two_chats, actions, |_| super::super::spaces::all());
    let names: Vec<&str> = seen.read.iter().map(|space| space.name.as_str()).collect();
    assert_eq!(names, ["Launch"]);
    let listed = rows(&seen.controls);
    let at = |name: &str| listed.iter().position(|row| *row == name);
    assert!(
        at("Launch") < at("Fix the parser bug")
            && at("Fix the parser bug") < at("work")
            && at("work") < at("Write release notes"),
        "the workspace, its agent, then the repository with the other agent: {listed:?}"
    );
    assert_eq!(
        listed.iter().filter(|row| **row == "Fix the parser bug").count(),
        1,
        "the moved agent lists once: {listed:?}"
    );
}
