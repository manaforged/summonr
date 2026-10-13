use crate::provider::scripted::{Reply, Scripted};
use crate::runtime::Loop;
use crate::tools::{Abort, AbortCode};

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

fn browser_parent(replies: Vec<Reply>) -> (Loop<Scripted>, [tempfile::TempDir; 2]) {
    let (mut parent, dirs) = parent(replies);
    let on = [(crate::modules::ModuleId::Browser, true)]
        .into_iter()
        .collect();
    parent
        .set_modules(&crate::modules::Switches::from_maps(
            &on,
            &Default::default(),
        ))
        .unwrap();
    (parent, dirs)
}

fn tool_names(loop_: &mut Loop<Scripted>) -> Vec<String> {
    loop_
        .advertised_tools()
        .tools
        .iter()
        .map(|definition| definition.function.name.clone())
        .collect()
}

fn args(assignment: &str, access: &str, background: bool) -> super::Args {
    serde_json::from_value(serde_json::json!({
        "assignment": assignment,
        "access": access,
        "background": background,
    }))
    .unwrap()
}

fn implement() -> super::Args {
    args("add the flag", "implement", false)
}

fn spawn_background(parent: &mut Loop<Scripted>) -> String {
    let budget = parent.task_budget(1).unwrap();
    parent
        .task_spawn(args("find the config loader", "research", true), &budget)
        .unwrap();
    parent.spawned.keys().next().cloned().unwrap()
}

fn abort(parent: &mut Loop<Scripted>, code: AbortCode, reason: &str) {
    let abort = Abort {
        code,
        reason: reason.to_string(),
    };
    parent.abort_with(&abort, |_| {}).unwrap();
}

#[tokio::test]
async fn a_child_gets_its_brief_and_its_own_tools() {
    let (mut parent, dirs) = browser_parent(vec![Reply::Text("hello"), Reply::Text("found it")]);
    parent.turn("hi").await.unwrap();
    assert!(tool_names(&mut parent).contains(&"browser".to_string()));
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent
        .task_start_in(
            args("find the config loader", "research", false),
            &budget,
            None,
        )
        .unwrap();
    assert_eq!(run.child.system_prompt, parent.system_prompt);
    let child_tools = tool_names(&mut run.child);
    assert!(
        !child_tools.contains(&"browser".to_string()),
        "{child_tools:?}"
    );
    assert!(!child_tools.iter().any(|name| name.starts_with("task")));
    assert!(
        run.assignment.starts_with("Harness: child."),
        "{}",
        run.assignment
    );
    assert!(
        run.assignment.contains(
            &dunce::canonicalize(dirs[0].path())
                .unwrap()
                .display()
                .to_string()
        ),
        "{}",
        run.assignment
    );
    assert!(run.assignment.ends_with("find the config loader"));
    let assignment = run.assignment.clone();
    run.child.turn(&assignment).await.unwrap();
}

#[tokio::test]
async fn an_implement_child_keeps_the_browser() {
    let (mut parent, _dirs) = browser_parent(vec![Reply::Text("done")]);
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent.task_start_in(implement(), &budget, None).unwrap();
    assert!(tool_names(&mut run.child).contains(&"browser".to_string()));
}

#[tokio::test]
async fn a_cancelled_turn_leaves_background_children_settleable_by_poll() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("ok"), Reply::Text("child done")]);
    parent.turn("hi").await.unwrap();
    let id = spawn_background(&mut parent);
    abort(&mut parent, AbortCode::Cancelled, "cancelled by user");
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
        std::path::Path::new("/w/app-children/1"),
    );
    assert_eq!(
        text,
        "In the current workspace (/w/app-children/1), fix /w/app-children/1/src/a.rs; leave /w/app-old alone"
    );
}

#[tokio::test]
async fn a_failed_turn_drops_the_updates_its_background_children_left_queued() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("ok"), Reply::Text("child done")]);
    parent.turn("hi").await.unwrap();
    let id = spawn_background(&mut parent);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    abort(&mut parent, AbortCode::Error, "provider failed");
    let late: Vec<_> = parent
        .child_events()
        .into_iter()
        .filter(
            |event| matches!(event, crate::tools::Event::Child { id: child, .. } if *child == id),
        )
        .collect();
    assert!(late.is_empty(), "{late:?}");
}
