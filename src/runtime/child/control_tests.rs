use super::*;
use crate::tools::Event;
use std::time::Duration;

async fn idle(parent: &mut Loop<Scripted>, id: &str, expected: &str) -> Vec<Event> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = Vec::new();
        loop {
            parent.drain(&mut events, &mut |_| {});
            let child = &parent.spawned[id];
            if child.state.borrow().terminal() && child.last.contains(expected) {
                return events;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the named child did not report the expected idle outcome")
}

fn has_failure(events: &[Event], id: &str, expected: &str) -> bool {
    events.iter().any(|event| {
        matches!(event, Event::Child { id: child, event }
            if child == id && matches!(event.as_ref(), Event::Aborted { reason, .. }
                if reason.contains(expected)))
    })
}

#[tokio::test]
async fn an_initial_failure_reaches_poll_and_events_before_waiting_for_mail() {
    let (mut parent, _dirs) = parent(vec![Reply::Fail("initial request rejected")]);
    let budget = parent
        .task_budget(1)
        .expect("the fixture has a child budget");
    parent
        .task_spawn(named("read the assignment", "initial-failure"), &budget)
        .expect("the named child operation must succeed");
    let id = parent
        .spawned
        .keys()
        .next()
        .expect("one named child was spawned")
        .clone();
    let events = idle(&mut parent, &id, "initial request rejected").await;
    assert!(has_failure(&events, &id, "initial request rejected"));
    assert!(!parent.spawned[&id].handle.is_finished());
    let polled = parent
        .task_poll(&id, false, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
    assert!(polled.output.text.contains("state: idle"));
    assert!(polled.output.text.contains("initial request rejected"));
    assert!(!polled.output.text.contains("turn failed"));
    parent
        .task_poll(&id, true, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
}

#[tokio::test]
async fn a_failed_first_turn_recovers_by_mail_without_a_replacement_child() {
    const RESULT: &str = concat!(
        "recovered: the worker completed the assigned investigation and retained the measured ",
        "input, command, raw result, source revision, and interpretation for the parent. ",
        "The final evidence must remain visible beyond the running progress preview."
    );
    let (mut parent, _dirs) = parent(vec![
        Reply::Fail("first turn rejected"),
        Reply::Text(RESULT),
    ]);
    let budget = parent
        .task_budget(1)
        .expect("the fixture has a child budget");
    parent
        .task_spawn(named("read the assignment", "first-recovery"), &budget)
        .expect("the named child operation must succeed");
    let id = parent
        .spawned
        .keys()
        .next()
        .expect("one named child was spawned")
        .clone();
    let failed = idle(&mut parent, &id, "first turn rejected").await;
    crate::mail::Mailbox::wake(&id, "retry the assignment")
        .expect("the named child operation must succeed");
    let recovered = idle(&mut parent, &id, "recovered").await;
    assert!(has_failure(&failed, &id, "first turn rejected"));
    assert!(!has_failure(&recovered, &id, "first turn rejected"));
    assert_eq!(parent.spawned.len(), 1);
    assert_eq!(parent.spawned[&id].record.id.to_string(), id);
    let polled = parent
        .task_poll(&id, false, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
    assert!(
        polled.output.text.contains(RESULT),
        "{}",
        polled.output.text
    );
    parent
        .task_poll(&id, true, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
}

#[tokio::test]
async fn mailed_failures_use_the_same_diagnostic_path_and_can_recover() {
    let (mut parent, _dirs) = parent(vec![
        Reply::Text("first turn done"),
        Reply::Fail("mailed request rejected"),
        Reply::Text("second recovery"),
    ]);
    let budget = parent
        .task_budget(1)
        .expect("the fixture has a child budget");
    parent
        .task_spawn(named("read the assignment", "mail-recovery"), &budget)
        .expect("the named child operation must succeed");
    let id = parent
        .spawned
        .keys()
        .next()
        .expect("one named child was spawned")
        .clone();
    idle(&mut parent, &id, "first turn done").await;
    crate::mail::Mailbox::wake(&id, "continue the assignment")
        .expect("the named child operation must succeed");
    let failed = idle(&mut parent, &id, "mailed request rejected").await;
    assert!(has_failure(&failed, &id, "mailed request rejected"));
    let polled = parent
        .task_poll(&id, false, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
    assert!(polled.output.text.contains("mailed request rejected"));
    crate::mail::Mailbox::wake(&id, "try the next assignment")
        .expect("the named child operation must succeed");
    idle(&mut parent, &id, "second recovery").await;
    assert_eq!(parent.spawned.len(), 1);
    let stopped = parent
        .task_poll(&id, true, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
    let record: crate::child::ChildRecord =
        serde_json::from_str(&stopped.output.text).expect("the stop receipt is a child record");
    assert!(matches!(record.state, crate::child::ChildState::Aborted(_)));
    assert_eq!(
        record
            .state
            .result()
            .expect("a stopped child has a result")
            .diagnostic
            .as_deref(),
        Some("killed by the parent")
    );
    assert!(!parent.spawned.contains_key(&id));
}

#[tokio::test]
async fn stopping_a_running_named_child_settles_the_same_identity() {
    let start = call(
        "owned-job",
        "run_command",
        &serde_json::json!({"command": "sleep 30", "background": true}),
    );
    let (mut parent, _dirs) = parent(vec![Reply::Call(start.clone()), Reply::Pending]);
    parent
        .tools
        .execute_outcome(&start)
        .await
        .expect("the parent job started");
    let requests = parent.provider.sent();
    let budget = parent
        .task_budget(1)
        .expect("the fixture has a child budget");
    let mut assignment = named("wait for work", "stop-running");
    assignment.access = crate::child::Access::Implement;
    parent
        .task_spawn(assignment, &budget)
        .expect("the named child operation must succeed");
    tokio::time::timeout(Duration::from_secs(5), async {
        while requests.lock().expect("captured requests").len() < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the child started its job before cancellation");
    let id = parent
        .spawned
        .keys()
        .next()
        .expect("one named child was spawned")
        .clone();
    let job = crate::jobs::since(parent.workspace(), 0, Some(&id))
        .into_iter()
        .find(|record| record.session.as_deref() == Some(id.as_str()))
        .expect("the running child owns its background job");
    let stopped = parent
        .task_poll(&id, true, &mut Vec::new(), &mut |_| {})
        .await
        .expect("the named child operation must succeed");
    assert!(stopped.output.success);
    let child_status =
        crate::jobs::status(parent.workspace(), &job).expect("the child job has a final status");
    let parent_jobs = parent
        .tools
        .stop_jobs()
        .expect("the parent job is cleaned up");
    crate::tools::command::stop_jobs(parent.workspace(), &id)
        .expect("any remaining child fixture jobs are cleaned up");
    assert_eq!(child_status.state, crate::jobs::JobState::Killed);
    assert_eq!(
        parent_jobs.len(),
        1,
        "child cancellation stopped the parent job"
    );
    let record: crate::child::ChildRecord =
        serde_json::from_str(&stopped.output.text).expect("the stop receipt is a child record");
    assert_eq!(record.id.to_string(), id);
    assert!(matches!(record.state, crate::child::ChildState::Aborted(_)));
    let diagnostic = record
        .state
        .result()
        .expect("a stopped child has a result")
        .diagnostic
        .as_deref()
        .expect("cancellation reports its reason and cleanup");
    assert!(
        diagnostic.starts_with("killed by the parent"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains(&job.id), "{diagnostic}");
    assert!(!parent.spawned.contains_key(&id));
}
