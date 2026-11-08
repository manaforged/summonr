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
        Step::Click("Add project"),
        Step::Click("New project"),
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

#[test]
fn a_repository_added_with_no_agent_lists_at_once() {
    if !in_window("spaces::a_repository_added_with_no_agent_lists_at_once") {
        return;
    }
    let root = tempfile::tempdir().expect("folder");
    let shop = root.path().join("shop");
    std::fs::create_dir(&shop).expect("shop");
    sigil::app::dialog::mock::push_path(Some(shop));
    let seen = steps(
        two_chats,
        vec![Step::Click("Add project"), Step::Click("Add repository…")],
        |_| (),
    );
    let listed = rows(&seen.controls);
    assert!(listed.contains(&"shop"), "the new repository lists: {listed:?}");
    assert!(
        listed.contains(&"Fix the parser bug") && listed.contains(&"Write release notes"),
        "the other agents still list: {listed:?}"
    );
}
