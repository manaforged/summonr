use crate::provider::scripted::{Reply, Scripted};
use crate::runtime::Loop;

#[tokio::test]
async fn a_child_gets_its_brief_and_its_own_tools() {
    let provider = Scripted::new([Reply::Text("hello"), Reply::Text("found it")]);
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .unwrap();
    let mut parent = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    let browser_on: std::collections::BTreeMap<crate::modules::ModuleId, bool> =
        [(crate::modules::ModuleId::Browser, true)]
            .into_iter()
            .collect();
    parent
        .set_modules(&crate::modules::Switches::from_maps(
            &browser_on,
            &Default::default(),
        ))
        .unwrap();
    parent.turn("hi").await.unwrap();
    assert!(
        parent
            .advertised_tools()
            .tools
            .iter()
            .any(|definition| definition.function.name == "browser")
    );
    let args = serde_json::from_value(serde_json::json!({
        "assignment": "find the config loader",
        "access": "research",
    }))
    .unwrap();
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent.task_start_in(args, &budget, None).unwrap();
    assert_eq!(run.child.system_prompt, parent.system_prompt);
    let child_tools = run.child.advertised_tools();
    assert!(
        !child_tools
            .tools
            .iter()
            .any(|definition| definition.function.name == "browser"),
        "{}",
        child_tools
            .tools
            .iter()
            .map(|definition| definition.function.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert!(
        !child_tools.tools.iter().any(|definition| definition
            .function
            .name
            .as_str()
            .starts_with("task"))
    );
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
}

#[tokio::test]
async fn an_implement_child_keeps_the_browser() {
    let provider = Scripted::new([Reply::Text("done")]);
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .unwrap();
    let mut parent = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    let browser_on: std::collections::BTreeMap<crate::modules::ModuleId, bool> =
        [(crate::modules::ModuleId::Browser, true)]
            .into_iter()
            .collect();
    parent
        .set_modules(&crate::modules::Switches::from_maps(
            &browser_on,
            &Default::default(),
        ))
        .unwrap();
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent
        .task_start_in(
            serde_json::from_value(serde_json::json!({
                "assignment": "add the flag",
                "access": "implement",
            }))
            .unwrap(),
            &budget,
            None,
        )
        .unwrap();
    assert!(
        run.child
            .advertised_tools()
            .tools
            .iter()
            .any(|definition| definition.function.name == "browser")
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
async fn a_cancelled_turn_leaves_background_children_settleable_by_poll() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("ok"), Reply::Text("child done")]);
    parent.turn("hi").await.unwrap();
    let args = serde_json::from_value(serde_json::json!({
        "assignment": "find the config loader",
        "access": "research",
        "background": true,
    }))
    .unwrap();
    let budget = parent.task_budget(1).unwrap();
    parent.task_spawn(args, &budget).unwrap();
    let id = parent.spawned.keys().next().cloned().unwrap();
    parent
        .abort_with(
            &crate::tools::Abort {
                code: crate::tools::AbortCode::Cancelled,
                reason: "cancelled by user".to_string(),
            },
            |_| {},
        )
        .unwrap();
    assert!(parent.spawned.contains_key(&id));
    let running = id
        .parse::<crate::durable::id::SessionId>()
        .ok()
        .and_then(|parsed| parent.session.child(parsed))
        .map(|record| record.state);
    assert!(matches!(running, Some(crate::child::ChildState::Running)));
    parent
        .await_children(
            &mut Vec::new(),
            &mut |_| {},
            &mut crate::provider::Usage::default(),
        )
        .await
        .unwrap();
    let execution = parent
        .task_poll(&id, false, &mut Vec::new(), &mut |_| {})
        .await
        .unwrap();
    assert!(execution.output.success);
}

#[tokio::test]
async fn a_child_of_a_goal_session_works_in_plain_mode_and_finishes_its_turn() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("done")]);
    parent.select_goal("ship it", &[]).unwrap();
    parent.set_mode(crate::RunMode::Goal);
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent.task_start_in(implement(), &budget, None).unwrap();
    assert_eq!(run.child.mode(), crate::RunMode::Auto);
    let assignment = run.assignment.clone();
    run.child.turn(&assignment).await.unwrap();
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

#[tokio::test]
async fn a_failed_turn_drops_the_updates_its_background_children_left_queued() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("ok"), Reply::Text("child done")]);
    parent.turn("hi").await.unwrap();
    let args = serde_json::from_value(serde_json::json!({
        "assignment": "find the config loader",
        "access": "research",
        "background": true,
    }))
    .unwrap();
    let budget = parent.task_budget(1).unwrap();
    parent.task_spawn(args, &budget).unwrap();
    let id = parent.spawned.keys().next().cloned().unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    parent
        .abort_with(
            &crate::tools::Abort {
                code: crate::tools::AbortCode::Error,
                reason: "provider failed".to_string(),
            },
            |_| {},
        )
        .unwrap();
    let late: Vec<_> = parent
        .child_events()
        .into_iter()
        .filter(
            |event| matches!(event, crate::tools::Event::Child { id: child, .. } if *child == id),
        )
        .collect();
    assert!(late.is_empty(), "{late:?}");
}
