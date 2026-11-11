use sigil::events::{Key, ModifiersState};
use sigil::prelude::Role;

use super::harness::{Step, named, steps, typed};
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
    let mut actions = vec![Step::Click("Add project"), Step::Click("New project")];
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
        listed
            .iter()
            .filter(|row| **row == "Fix the parser bug")
            .count(),
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
    assert!(
        listed.contains(&"shop"),
        "the new repository lists: {listed:?}"
    );
    assert!(
        listed.contains(&"Fix the parser bug") && listed.contains(&"Write release notes"),
        "the other agents still list: {listed:?}"
    );
}

#[test]
fn a_repository_added_to_a_workspace_lists_under_it_with_its_agents() {
    if !in_window("spaces::a_repository_added_to_a_workspace_lists_under_it_with_its_agents") {
        return;
    }
    sigil::app::dialog::mock::push_path(Some("/work".into()));
    let seen = steps(
        two_chats,
        vec![
            Step::Click("Add project"),
            Step::Click("New project"),
            Step::Key(named(Key::Escape, ModifiersState::empty())),
            Step::RightClick("New project"),
            Step::Click("Add repository…"),
        ],
        |_| super::super::spaces::all(),
    );
    assert_eq!(seen.read.len(), 1, "one workspace");
    let listed = rows(&seen.controls);
    let at = |name: &str| listed.iter().position(|row| *row == name);
    let header = at("New project").expect("the workspace lists");
    assert_eq!(
        listed.get(header + 1),
        Some(&"work"),
        "the workspace's repository comes right under it, before any other: {listed:?}"
    );
    assert!(
        at("work") < at("Fix the parser bug")
            && at("Fix the parser bug") < at("Write release notes"),
        "the workspace, its repository, then the repository's agents: {listed:?}"
    );
    assert_eq!(
        listed.iter().filter(|row| **row == "work").count(),
        1,
        "the repository lists once: {listed:?}"
    );
}
