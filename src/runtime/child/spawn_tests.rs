use std::sync::Arc;

use crate::provider::scripted::{Reply, Scripted};
use crate::runtime::Loop;

#[tokio::test]
async fn a_child_reuses_the_parent_prefix_and_gets_its_brief_in_the_first_message() {
    let provider = Scripted::new([Reply::Text("hello"), Reply::Text("found it")]);
    let prefixes = provider.prefixes();
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .unwrap();
    let mut parent = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    parent.turn("hi").await.unwrap();
    let parent_tools = parent.advertised_tools();
    let args = serde_json::from_value(serde_json::json!({
        "assignment": "find the config loader",
        "access": "research",
    }))
    .unwrap();
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent.task_start_in(args, &budget, None).unwrap();
    assert_eq!(run.child.system_prompt, parent.system_prompt);
    let child_tools = run.child.schema.clone().unwrap();
    assert!(Arc::ptr_eq(&child_tools, &parent_tools));
    assert!(
        run.assignment.starts_with("Harness: child."),
        "{}",
        run.assignment
    );
    assert!(
        run.assignment
            .contains(&workspace.path().display().to_string()),
        "{}",
        run.assignment
    );
    assert!(run.assignment.ends_with("find the config loader"));
    let assignment = run.assignment.clone();
    run.child.turn(&assignment).await.unwrap();
    let prefixes = prefixes.lock().unwrap();
    assert_eq!(prefixes.len(), 2);
    assert_eq!(
        prefixes[1], prefixes[0],
        "the child's first request missed the parent prefix"
    );
}

fn parent(replies: Vec<Reply>) -> (Loop<Scripted>, [tempfile::TempDir; 2]) {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .unwrap();
    let parent = Loop::open(
        Scripted::new(replies),
        "m",
        workspace.path(),
        "system",
        driver,
    )
    .unwrap();
    (parent, [workspace, home])
}

fn implement() -> super::Args {
    serde_json::from_value(serde_json::json!({
        "assignment": "add the flag",
        "access": "implement",
    }))
    .unwrap()
}

#[tokio::test]
async fn a_child_of_a_goal_session_works_in_plain_mode_and_finishes_its_turn() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("done")]);
    parent.select_goal("ship it", &[]).unwrap();
    parent.set_mode(crate::RunMode::Goal).unwrap();
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent.task_start_in(implement(), &budget, None).unwrap();
    assert_eq!(run.child.mode(), crate::RunMode::Auto);
    let assignment = run.assignment.clone();
    run.child.turn(&assignment).await.unwrap();
}

#[tokio::test]
async fn a_child_of_a_plan_session_cannot_write() {
    let (parent, _dirs) = parent(Vec::new());
    let mut parent = parent.with_plan();
    let budget = parent.task_budget(1).unwrap();
    let run = parent.task_start_in(implement(), &budget, None).unwrap();
    assert!(run.child.tools.is_read_only());
}

#[test]
fn an_assignment_names_the_childs_checkout_not_the_parent() {
    let text = super::super::brief::rebase_paths(
        "In the current workspace (/w/app), fix /w/app/src/a.rs; leave /w/app-old alone",
        std::path::Path::new("/w/app"),
        std::path::Path::new("/w/app-deltas/1"),
    );
    assert_eq!(
        text,
        "In the current workspace (/w/app-deltas/1), fix /w/app-deltas/1/src/a.rs; leave /w/app-old alone"
    );
}
