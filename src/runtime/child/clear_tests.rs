use super::*;

#[tokio::test]
async fn stopping_and_clearing_a_child_keeps_its_receipt() {
    let (mut parent, _dirs) = parent(vec![Reply::Pending]);
    let budget = parent.task_budget(1).expect("child budget");
    parent
        .task_spawn(named("wait", "stop-clear"), &budget)
        .expect("start child");
    let id = parent.spawned.keys().next().expect("spawned child").clone();
    let mut events = Vec::new();
    let clear = call(
        "stop-clear",
        "task",
        &serde_json::json!({
            "child": id, "kill": true, "clear": true
        }),
    );
    let result = parent
        .task_execute(&clear, &mut events, &mut |_| {})
        .await
        .expect("stop and clear");
    assert!(result.output.success);
    assert!(!parent.spawned.contains_key(&id));
    for _ in 0..2 {
        let clear = call(
            "clear-again",
            "task",
            &serde_json::json!({"child": id, "clear": true}),
        );
        assert!(
            parent
                .task_execute(&clear, &mut events, &mut |_| {})
                .await
                .expect("repeat clear")
                .output
                .success
        );
    }
    let polled = parent
        .task_poll(&id, false, &mut events, &mut |_| {})
        .await
        .expect("retained receipt");
    assert!(polled.output.text.contains("killed by the parent"));
    assert!(polled.output.text.contains(&id));
}

#[tokio::test]
async fn clearing_requires_a_child_and_rejects_an_assignment() {
    let (mut parent, _dirs) = parent(Vec::new());
    for (value, reason) in [
        (serde_json::json!({"clear": true}), "require child"),
        (
            serde_json::json!({"child": "", "clear": true}),
            "child id is empty",
        ),
        (
            serde_json::json!({"child": "unknown", "clear": true, "assignment": "run"}),
            "either child or assignment",
        ),
    ] {
        let request = call("invalid-clear", "task", &value);
        let Err(error) = parent
            .task_execute(&request, &mut Vec::new(), &mut |_| {})
            .await
        else {
            panic!("invalid clear must fail: {reason}");
        };
        assert!(error.to_string().contains(reason), "{error}");
        assert!(parent.spawned.is_empty());
    }
}

#[tokio::test]
async fn clearing_a_child_keeps_it_running_and_pollable() {
    let (mut parent, _dirs) = parent(vec![Reply::Pending]);
    let budget = parent.task_budget(1).expect("child budget");
    parent
        .task_spawn(named("wait for work", "clear-running"), &budget)
        .expect("start a named child");
    let id = parent.spawned.keys().next().expect("spawned child").clone();
    let mut events = Vec::new();
    let clear = call(
        "clear",
        "task",
        &serde_json::json!({"child": id, "clear": true}),
    );
    let result = parent.task_execute(&clear, &mut events, &mut |_| {}).await;
    let retained = parent.spawned.contains_key(&id);
    parent
        .task_poll(&id, true, &mut Vec::new(), &mut |_| {})
        .await
        .expect("clean up the running child");
    let result = result.expect("clear a known child without stopping it");
    assert!(result.output.success);
    assert!(retained, "clear cancelled the child");
    assert!(
        events.iter().any(|event| {
            let value = serde_json::to_value(event).expect("serialize event");
            value["id"] == id && value["event"]["type"] == "dismissed"
        }),
        "clear did not reach the interface"
    );
    let polled = parent
        .task_poll(&id, false, &mut Vec::new(), &mut |_| {})
        .await
        .expect("history remains pollable after clearing");
    assert!(polled.output.text.contains(&id));
}

#[tokio::test]
async fn clearing_an_unknown_child_never_hides_a_card() {
    let (mut parent, _dirs) = parent(Vec::new());
    let clear = call(
        "clear",
        "task",
        &serde_json::json!({
            "child": "01a104ac000000000000000000001111", "clear": true
        }),
    );
    let mut events = Vec::new();
    let result = parent.task_execute(&clear, &mut events, &mut |_| {}).await;
    let Err(error) = result else {
        panic!("unknown child must fail");
    };
    assert!(error.to_string().contains("unknown child"));
    assert!(!events.iter().any(|event| {
        serde_json::to_value(event).expect("serialize event")["event"]["type"] == "dismissed"
    }));
}
