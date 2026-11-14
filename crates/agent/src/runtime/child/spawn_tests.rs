use crate::provider::scripted::{Reply, Scripted};
use crate::provider::{ToolCall, ToolFunction, ToolKind};
use crate::runtime::Loop;
use crate::tools::{Abort, AbortCode};

#[path = "clear_tests.rs"]
mod clear;
#[path = "control_tests.rs"]
mod control;
#[path = "delta_tests.rs"]
mod delta;
#[path = "model_tests.rs"]
mod model;

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

fn named(assignment: &str, name: &str) -> super::Args {
    serde_json::from_value(serde_json::json!({
        "assignment": assignment,
        "access": "research",
        "background": true,
        "name": name,
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
        child_tools.contains(&"browser".to_string()),
        "{child_tools:?}"
    );
    assert!(!child_tools.iter().any(|name| name.starts_with("task")));
    assert!(
        run.assignment
            .starts_with("Harness: child. Finish only the assigned task."),
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
    let execution = parent.task_poll(&id, false, &mut |_| {}).await.unwrap();
    assert!(execution.output.success);
}

#[tokio::test]
async fn a_child_of_a_goal_session_works_in_plain_mode_and_finishes_its_turn() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("done")]);
    parent.select_goal("ship it", &[]).unwrap();
    parent.set_mode(crate::WorkKind::Goal);
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent.task_start_in(implement(), &budget, None).unwrap();
    assert_eq!(run.child.mode(), crate::WorkKind::Single);
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
async fn a_failed_turn_keeps_its_background_children() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("ok"), Reply::Text("child done")]);
    parent.turn("hi").await.unwrap();
    let id = spawn_background(&mut parent);
    abort(&mut parent, AbortCode::Error, "provider failed");
    assert!(parent.spawned.contains_key(&id));
}

fn call(id: &str, name: &str, arguments: &serde_json::Value) -> crate::provider::ToolCall {
    crate::provider::ToolCall {
        id: id.to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            checked: Default::default(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    }
}

#[tokio::test]
async fn task_calls_beside_a_todo_update_start_together() {
    let research = |id, assignment| {
        call(
            id,
            "task",
            &serde_json::json!({ "assignment": assignment, "access": "research" }),
        )
    };
    let round = vec![
        call(
            "t0",
            "todo",
            &serde_json::json!({ "items": [{ "text": "look around", "status": "done" }] }),
        ),
        research("t1", "find the config loader"),
        research("t2", "find the session store"),
    ];
    let (mut parent, _dirs) = parent(vec![
        Reply::Calls(round),
        Reply::Text("the loader"),
        Reply::Text("the store"),
        Reply::Text("both found"),
    ]);
    let turn = parent.turn("look around").await.unwrap();
    let spawned: Vec<bool> = turn
        .events
        .iter()
        .filter_map(|event| match event {
            crate::tools::Event::Child { event, .. } => {
                Some(matches!(**event, crate::tools::Event::Spawned { .. }))
            }
            _ => None,
        })
        .collect();
    assert_eq!(spawned.get(..2), Some(&[true, true][..]), "{spawned:?}");
}

#[tokio::test]
async fn a_job_wait_settles_a_child_that_finished_before_it_and_then_waits_on_the_job() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("ok"), Reply::Text("child done")]);
    parent.turn("hi").await.unwrap();
    let id = spawn_background(&mut parent);
    let started = std::time::Instant::now();
    while !parent.spawned[&id].handle.is_finished() {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let job = parent
        .tools
        .execute_outcome(&call(
            "c1",
            "run_command",
            &serde_json::json!({"command": "sleep 30", "background": true}),
        ))
        .await
        .unwrap()
        .text;
    let job = job
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .unwrap()
        .to_string();
    let mut events = Vec::new();
    let waited = std::time::Instant::now();
    let (output, _) = parent
        .run_or_wake(
            &call("c2", "run_command", &serde_json::json!({"job": job})),
            &mut events,
            &mut |_| {},
        )
        .await
        .unwrap();
    drop(parent.tools.stop_jobs());
    let text = output.unwrap().text;
    assert!(!text.starts_with("returned early"), "{text}");
    assert!(text.contains("state: running"), "{text}");
    assert!(waited.elapsed() >= std::time::Duration::from_millis(900));
    assert!(!parent.spawned.contains_key(&id));
}

#[tokio::test]
async fn a_research_child_can_run_a_foreground_command() {
    let (mut parent, dirs) = parent(vec![Reply::Text("done")]);
    let budget = parent.task_budget(1).unwrap();
    let run = parent
        .task_start_in(args("hold", "research", false), &budget, None)
        .unwrap();
    let output = run
        .child
        .execute_for_test(&ToolCall {
            id: "t1".to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
                name: "run_command".to_string(),
                arguments: serde_json::json!({"command": "echo ok"}).to_string(),
            },
            continuation: None,
        })
        .await
        .unwrap();
    assert!(output.success, "{}", output.text);
    assert!(output.text.contains("ok"), "{}", output.text);
    drop(dirs);
}

#[cfg(unix)]
#[tokio::test]
async fn incoming_input_interrupts_a_child_and_leaves_main_session_jobs_alone() {
    let start = crate::provider::ToolCall {
        id: "start".to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": "sleep 30", "background": true}).to_string(),
        },
        continuation: None,
    };
    let (mut parent, _dirs) = parent(vec![Reply::Text("first")]);
    let budget = parent.task_budget(1).unwrap();
    let name = format!("steered-{}", parent.session_id());
    let mut run = parent
        .task_start_in(named("wait", &name), &budget, None)
        .unwrap();
    parent.tools.execute_outcome(&start).await.unwrap();
    assert_eq!(parent.steered("lead"), "lead");
    assert_eq!(parent.tools.stop_jobs().unwrap().len(), 1);
    assert!(run.child.input_interrupts());
    let steering = std::sync::Arc::new(crate::runtime::Steering::default());
    run.child.steer_with(std::sync::Arc::clone(&steering));
    run.child.tools.set_read_only(false);
    let job = run
        .child
        .tools
        .execute_outcome(&start)
        .await
        .expect("implementation children inherit background jobs");
    assert!(job.success, "{}", job.text);
    let job_id = job
        .text
        .lines()
        .find_map(|line| line.strip_prefix("started job "))
        .expect("the child command returned its job id")
        .to_string();
    let workspace = run.child.workspace().to_path_buf();
    let command = ToolCall {
        id: "stopped".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": "touch child-started; sleep 30; touch child-escaped"
            })
            .to_string(),
        },
        continuation: None,
    };
    let sender = async {
        while !workspace.join("child-started").exists() {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        steering
            .lock()
            .unwrap()
            .push_back(crate::provider::UserInput::from("stop now"));
        steering.notify();
    };
    let mut events = Vec::new();
    let mut emit = |_: &crate::tools::Event| {};
    let waited = run.child.run_or_wake(&command, &mut events, &mut emit);
    let (outcome, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(waited, sender)
    })
    .await
    .unwrap();
    let (output, _) = outcome.unwrap();
    let Err(error) = output else {
        panic!("the command must be interrupted");
    };
    assert!(
        error
            .to_string()
            .contains("interrupted by an incoming message")
    );
    assert!(!workspace.join("child-escaped").exists());
    let record = crate::jobs::load(&workspace, &job_id).expect("the child job is recorded");
    assert_eq!(
        crate::jobs::status(&workspace, &record)
            .expect("the child job has a status")
            .state,
        crate::jobs::JobState::Killed
    );
}

#[tokio::test]
async fn two_messages_to_the_same_name_stay_on_one_session() {
    let (mut parent, _dirs) = parent(vec![Reply::Text("first"), Reply::Text("second")]);
    let budget = parent.task_budget(1).unwrap();
    let name = format!("climb-{}", parent.session_id());
    let mut run = parent
        .task_start_in(named("look at the loader", &name), &budget, None)
        .unwrap();
    let id = run.child.session_id();
    assert_eq!(run.child.name().as_deref(), Some(name.as_str()));
    assert!(
        run.child.mailbox.is_some(),
        "the child is not on the roster"
    );
    let agents = crate::mail::live(&crate::mail::dir().unwrap());
    let resolved = agents.iter().find(|agent| agent.answers_to(&name));
    assert_eq!(resolved.map(|agent| agent.id.as_str()), Some(id.as_str()));
    crate::mail::Mailbox::wake(&id, "port the win").unwrap();
    crate::mail::Mailbox::wake(&id, "port the next win").unwrap();
    let notes = run.child.mailbox.as_mut().unwrap().poll().unwrap();
    let text = notes
        .iter()
        .map(|note| note.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("port the win"), "{text}");
    assert!(text.contains("port the next win"), "{text}");
    assert_eq!(run.child.session_id(), id);
}

#[tokio::test]
async fn shared_lanes_publish_writes_without_finishing_or_reconciling() {
    let (mut parent, _dirs) = parent(Vec::new());
    parent.tools.set_unrestricted();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&parent.workspace)
            .status()
            .unwrap()
            .success()
    );
    let budget = parent.task_budget(5).unwrap();
    let mut lanes = Vec::new();
    for index in 0..5 {
        let args = serde_json::from_value(serde_json::json!({
            "assignment": format!("write lane-{index}.txt"),
            "access": "implement",
            "workspace": "shared",
            "background": true,
        }))
        .unwrap();
        lanes.push(parent.task_start_in(args, &budget, None).unwrap());
    }
    for (index, lane) in lanes.iter().enumerate() {
        let path = format!("lane-{index}.txt");
        let call = call(
            "write",
            "write",
            &serde_json::json!({
                "path": path,
                "content": "verified win",
            }),
        );
        lane.child.tools.execute_for_test(&call).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(parent.workspace.join(path)).unwrap(),
            "verified win"
        );
    }
    drop(lanes);
    assert_eq!(
        std::fs::read_to_string(parent.workspace.join("lane-0.txt")).unwrap(),
        "verified win"
    );
}

#[tokio::test]
async fn a_named_child_keeps_its_first_text_and_does_not_block_the_parent() {
    let (mut parent, _dirs) = parent(vec![
        Reply::Text("landed"),
        Reply::Text("unnamed done"),
        Reply::Text("ported"),
    ]);
    let budget = parent.task_budget(1).unwrap();
    parent
        .task_spawn(named("find the loader", "climb-b"), &budget)
        .unwrap();
    let id = parent.spawned.keys().next().cloned().unwrap();
    let started = std::time::Instant::now();
    while !parent.spawned[&id].state.borrow().terminal() {
        parent.drain(&mut |_| {});
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "named child did not record its first turn"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    parent.drain(&mut |_| {});
    assert!(
        parent.spawned[&id].last.contains("landed"),
        "parent missing the first text: {}",
        parent.spawned[&id].last
    );
    parent
        .task_spawn(args("finish", "research", true), &budget)
        .unwrap();
    let unnamed = parent
        .spawned
        .keys()
        .find(|key| key.as_str() != id)
        .cloned()
        .unwrap();
    while !parent.spawned[&unnamed].handle.is_finished() {
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let running = parent
        .await_children(
            &mut Vec::new(),
            &mut |_| {},
            &mut crate::provider::Usage::default(),
        )
        .await
        .unwrap();
    assert!(
        !running.iter().any(|line| line.contains(&id)),
        "judge_work would skip: {running:?}"
    );
    assert!(!parent.spawned.contains_key(&unnamed));
    crate::mail::Mailbox::wake(&id, "port the win").unwrap();
    let sent = parent.provider.sent();
    while !sent.lock().unwrap().iter().any(|messages| {
        messages
            .iter()
            .any(|message| message.content.contains("port the win"))
    }) {
        parent.drain(&mut |_| {});
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the second mail did not become a turn"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(parent.spawned.contains_key(&id));
    assert_eq!(parent.spawned[&id].record.id.to_string(), id);
    parent
        .kill_spawned(
            crate::runtime::child::Kill::TurnEnded,
            &mut crate::provider::Usage::default(),
        )
        .await
        .unwrap();
    assert_eq!(parent.spawned[&id].record.id.to_string(), id);
}

#[tokio::test]
async fn a_child_runs_with_the_skill_set_of_its_lead() {
    let (mut parent, dirs) = parent(vec![Reply::Text("done")]);
    let skill = dirs[0].path().join(".summonr").join("skills").join("demo");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "# Demo\n").unwrap();
    let budget = parent.task_budget(1).unwrap();
    let mut open = parent
        .task_start_in(args("look around", "research", false), &budget, None)
        .unwrap();
    assert!(tool_names(&mut open.child).contains(&"skill".to_string()));
    parent
        .choose_skills(crate::skills::Set::from_sets(&std::collections::BTreeMap::new(), "none").unwrap())
        .unwrap();
    let budget = parent.task_budget(1).unwrap();
    let mut run = parent
        .task_start_in(args("look around", "research", false), &budget, None)
        .unwrap();
    assert!(!tool_names(&mut run.child).contains(&"skill".to_string()));
    assert_eq!(run.child.skills().as_deref(), Some("none"));
}
