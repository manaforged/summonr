use std::sync::Arc;

use crate::provider::scripted::Scripted;
use crate::runtime::Loop;

#[test]
fn a_child_reuses_the_parent_prefix_and_gets_its_brief_in_the_first_message() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .unwrap();
    let mut parent =
        Loop::open(Scripted::new([]), "m", workspace.path(), "system", driver).unwrap();
    let parent_tools = parent.advertised_tools();
    let args = serde_json::from_value(serde_json::json!({
        "assignment": "find the config loader",
        "access": "research",
    }))
    .unwrap();
    let budget = parent.task_budget(1).unwrap();
    let run = parent.task_start_in(args, &budget, None).unwrap();
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
}
