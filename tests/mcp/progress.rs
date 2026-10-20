use serde_json::{Value, json};

use crate::support::{Model, Session, Step};

pub(crate) fn task_progress_reaches_the_next_provider_turn() {
    let session = Session::new();
    let initial = json!({"items": [
        {"text": "First task", "status": "active"},
        {"text": "Second task", "status": "pending"},
        {"text": "Third task", "status": "pending"},
        {"text": "Fourth task", "status": "pending"},
        {"text": "Fifth task", "status": "pending"},
        {"text": "Sixth task", "status": "pending"}
    ]});
    let mut updated = initial.clone();
    updated["items"][0]["status"] = json!("done");
    updated["items"][1]["status"] = json!("active");
    let model = Model::start(vec![
        Step::Call("todo", initial),
        Step::Call("todo", updated),
        Step::Text("checkpoint"),
    ]);
    let result = session.run(&model, "", &["--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let progress: Vec<Value> = result
        .stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSON event"))
        .filter(|event| event["type"] == "work")
        .collect();
    let done: Vec<usize> = progress
        .iter()
        .map(|event| {
            event["progress"]["todos"]
                .as_array()
                .expect("task list")
                .iter()
                .filter(|item| item["status"] == "done")
                .count()
        })
        .collect();
    assert_eq!(done.first(), Some(&0), "{}", result.stdout);
    assert_eq!(done.last(), Some(&1), "{}", result.stdout);
    let resumed = Model::start(vec![Step::Text("resumed")]);
    let result = session.run(&resumed, "", &["-c", "--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let requests = resumed.requests();
    let input = requests[0]["messages"]
        .as_array()
        .expect("sent conversation")
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .and_then(|message| message["content"].as_str())
        .expect("current user input");
    assert!(input.contains("✓ First task"), "{input}");
    assert!(input.contains("● Second task"), "{input}");
}
