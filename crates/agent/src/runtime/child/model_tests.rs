use super::*;
use crate::child::{Access, ChildRecord};
use crate::provider::{Client, Effort, Usage};
use crate::runtime::child::Execution;
use crate::tools::Event;
use summonr_core::error::Result;

const TERRA: &str = "gpt-6.1-terra";
const SOL: &str = "gpt-6.1-sol";
const ROLES: [(&str, &str, &str, Access); 7] = [
    ("native-model-dl", TERRA, "xhigh", Access::Implement),
    ("native-model-pf", TERRA, "xhigh", Access::Implement),
    ("native-model-moe", TERRA, "xhigh", Access::Implement),
    ("native-model-attn", TERRA, "xhigh", Access::Implement),
    ("native-model-proj", TERRA, "xhigh", Access::Implement),
    ("native-model-monitor", TERRA, "xhigh", Access::Research),
    ("native-model-research", SOL, "medium", Access::Research),
];

#[tokio::test]
async fn native_seven_agent_models_do_not_change_parent_or_saved_defaults() {
    let (mut parent, _dirs, _listener, endpoint) = loopback_parent().await;
    let config = crate::config::path().expect("test config path");
    let before = std::fs::read(&config).expect("read configured defaults");
    let mut events = Vec::new();
    let results = parent
        .tasks_execute(&requests(&endpoint), &mut events, &mut |_| {})
        .await;
    parent.drain(&mut |event| events.push(event.clone()));
    let records = parent
        .kill_spawned(
            crate::runtime::child::Kill::Because("offline probe finished"),
            &mut Usage::default(),
        )
        .await
        .expect("stop every offline child");
    let after = std::fs::read(&config).expect("read defaults after native launch");
    crate::endpoint::remove_server(&endpoint).expect("remove test endpoint");
    assert_eq!(parent.model, SOL);
    assert_eq!(parent.effort, Some(Effort::Medium));
    assert_eq!(before, after, "native launch mutated configured defaults");
    assert_eq!(records.len(), 7);
    assert!(parent.spawned.is_empty());
    assert_eq!(events.iter().filter(|event| matches!(event, Event::Child { event, .. } if matches!(event.as_ref(), Event::Spawned { .. }))).count(), 7);
    assert_choices(&records, results);
}

async fn loopback_parent() -> (
    Loop<Client>,
    [tempfile::TempDir; 2],
    std::net::TcpListener,
    String,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let url = format!(
        "http://{}/v1",
        listener.local_addr().expect("listener address")
    );
    let endpoint = format!("native-model-test-{}", std::process::id());
    crate::endpoint::add_server(&endpoint, Some(&url), Some(SOL), false)
        .await
        .expect("register loopback endpoint in the test home");
    let workspace = tempfile::tempdir().expect("test workspace");
    let home = tempfile::tempdir().expect("session home");
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), SOL, "system")
            .expect("session driver");
    let provider = Client::new("", &url).expect("loopback client");
    let mut parent = Loop::open(provider, SOL, workspace.path(), "system", driver).expect("parent");
    parent.effort = Some(Effort::Medium);
    (parent, [workspace, home], listener, endpoint)
}

fn requests(endpoint: &str) -> Vec<ToolCall> {
    ROLES.iter().map(|(name, model, effort, access)| call(name, "task", &serde_json::json!({
        "name": name, "assignment": "Offline child selection probe", "workspace": "shared",
        "background": true, "access": access, "endpoint": endpoint,
        "model": model, "effort": effort
    }))).collect()
}

fn assert_choices(records: &[ChildRecord], results: Vec<Result<Execution>>) {
    assert_eq!(results.len(), ROLES.len());
    for ((name, model, effort, access), result) in ROLES.iter().zip(results) {
        let result = result.expect("explicit native model selection");
        assert!(result.output.success, "{}", result.output.text);
        assert!(
            result.output.text.contains(&format!("model: {model}")),
            "{}",
            result.output.text
        );
        assert!(
            result.output.text.contains(&format!("effort: {effort}")),
            "{}",
            result.output.text
        );
        let record = records
            .iter()
            .find(|record| record.name == *name)
            .expect("child record by name");
        assert_eq!(record.model, *model);
        assert_eq!(record.access, *access);
    }
}

#[tokio::test]
async fn native_model_selection_rejects_missing_endpoint_and_empty_model() {
    let (mut parent, _dirs) = parent(Vec::new());
    for (request, reason) in [
        (
            serde_json::json!({"assignment": "probe", "model": TERRA, "background": true}),
            "explicit endpoint",
        ),
        (
            serde_json::json!({"assignment": "probe", "endpoint": "unused", "model": " ", "background": true}),
            "must not be empty",
        ),
    ] {
        let request = call("invalid-model", "task", &request);
        let Err(error) = parent
            .task_execute(&request, &mut Vec::new(), &mut |_| {})
            .await
        else {
            panic!("invalid model selection must fail: {reason}");
        };
        assert!(error.to_string().contains(reason), "{error}");
        assert!(parent.spawned.is_empty());
    }
}
