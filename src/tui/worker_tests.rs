use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc::unbounded_channel;

use super::*;
use crate::prefs::Prefs;

fn worker() -> (Worker, UnboundedSender<Cmd>, PathBuf) {
    let (worker, tx, _ui, workspace) = worker_ui();
    (worker, tx, workspace)
}

fn worker_ui() -> (
    Worker,
    UnboundedSender<Cmd>,
    UnboundedReceiver<Msg>,
    PathBuf,
) {
    let workspace = std::env::temp_dir().join(format!(
        "mage-worker-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap_or_else(|error| panic!("{error}"));
    let loop_ = Loop::with_session(
        Client::new("", "http://localhost/v1").unwrap_or_else(|error| panic!("{error}")),
        "test",
        &workspace,
        "system",
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let (tx, rx) = unbounded_channel();
    let (ui, ui_rx) = unbounded_channel();
    let info = Info {
        version: "test".to_string(),
        model: "test".to_string(),
        relay: None,
        session: "test".to_string(),
        endpoint: "test".to_string(),
        prefs: Prefs::default(),
        plan: false,
        strict: false,
        workspace: workspace.display().to_string(),
        workflow: None,
        config: None,
    };
    (Worker::new(loop_, rx, ui, info), tx, ui_rx, workspace)
}

#[tokio::test]
async fn followup_runs_without_consuming_the_next_command() {
    let (mut worker, tx, workspace) = worker();
    worker.followups.push_back("queued".into());
    tx.send(Cmd::SetRounds(24))
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(matches!(worker.next().await, Some(Cmd::Submit(input)) if input.text == "queued"));
    assert!(matches!(worker.next().await, Some(Cmd::SetRounds(24))));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn deferred_setting_runs_before_a_queued_turn() {
    let (mut worker, _tx, workspace) = worker();
    worker.followups.push_back("queued".into());
    worker.deferred.push_back(Cmd::SetVerify(true));

    assert!(matches!(worker.next().await, Some(Cmd::SetVerify(true))));
    assert!(matches!(worker.next().await, Some(Cmd::Submit(input)) if input.text == "queued"));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn auto_reply_only_replaces_a_tick() {
    let (mut worker, tx, workspace) = worker();
    worker.pending_mail = true;
    worker.last_auto = Instant::now() - Duration::from_secs(11);
    tx.send(Cmd::SetMode(true))
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(matches!(worker.next().await, Some(Cmd::SetMode(true))));
    tx.send(Cmd::Tick).unwrap_or_else(|error| panic!("{error}"));
    assert!(matches!(worker.next().await, Some(Cmd::Mail)));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn endpoint_change_keeps_its_detected_context() {
    use crate::endpoint::{Endpoint, Wire};

    let (mut worker, _tx, workspace) = worker();
    let endpoint = Endpoint {
        name: "local".to_string(),
        model: "qwen-next".to_string(),
        base_url: Some("http://localhost/v1".to_string()),
        api_key: Some("key".to_string()),
        source: None,
        oauth: None,
        body: None,
        context_window: Some(65_536),
        idle_timeout_secs: None,
        effort: None,
        price: None,
        env: Default::default(),
        headers: Default::default(),
        wire: Wire::Completions,
    };

    worker.drive(Ok(endpoint)).await;

    assert_eq!(worker.loop_.model(), "qwen-next");
    assert_eq!(worker.endpoint, "local");
    assert_eq!(worker.loop_.context_window(), Some(65_536));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn the_card_seats_clears_and_reports_roles() {
    use crate::workflow::{Role, Seat};
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    worker.loop_.set_seat(
        Role::Plan,
        Client::new("", "http://localhost/v1").unwrap_or_else(|error| panic!("{error}")),
        "plan-model",
        None,
        None,
    );
    worker.workflow.seats.insert(
        Role::Plan,
        Seat {
            endpoint: "local".to_string(),
            model: Some("plan-model".to_string()),
        },
    );
    worker.send_workflow();
    let Some(Msg::Workflow { name, seats, .. }) = ui.recv().await else {
        panic!("expected a workflow message");
    };
    assert_eq!(name, None);
    assert_eq!(seats.len(), Role::ALL.len());
    assert_eq!(seats[1].as_deref(), Some("local\tplan-model"));
    assert_eq!(seats[2], None);

    worker.clear_role(Role::Plan);
    assert_eq!(worker.loop_.seat_model(Role::Plan), None);
    let Some(Msg::Workflow { seats, .. }) = ui.recv().await else {
        panic!("expected a workflow message");
    };
    assert_eq!(seats[1], None);
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

async fn system_line(ui: &mut UnboundedReceiver<Msg>) -> String {
    loop {
        match tokio::time::timeout(Duration::from_secs(5), ui.recv()).await {
            Ok(Some(Msg::System(text))) => return text,
            Ok(Some(Msg::Failed(text))) => panic!("worker failed: {text}"),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("no system line arrived"),
        }
    }
}

#[tokio::test]
async fn goal_clear_ends_the_active_goal() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/goal clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "no active goal");
    worker
        .loop_
        .select_goal("finish it", &[], 3)
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(worker.command("/goal clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "goal cleared — finish it");
    assert!(
        worker
            .loop_
            .active_goal()
            .unwrap_or_else(|error| panic!("{error}"))
            .is_none()
    );
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn steps_needs_a_readable_plan_and_clears_cleanly() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/steps").await, Some(true));
    assert!(system_line(&mut ui).await.starts_with("/steps PLAN.md"));
    assert_eq!(worker.command("/steps clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "no active steps run");
    assert_eq!(worker.command("/steps missing.md").await, Some(true));
    let failed = loop {
        match tokio::time::timeout(Duration::from_secs(5), ui.recv()).await {
            Ok(Some(Msg::Failed(text))) => break text,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("no failure arrived"),
        }
    };
    assert!(failed.contains("cannot read"), "{failed}");
    worker
        .loop_
        .select_steps("## Steps\n\n1. One.\n2. Two.\n", &[])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(worker.command("/steps clear").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "steps cleared");
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn effort_and_name_take_arguments_in_the_interface() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();
    assert_eq!(worker.command("/effort low").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "effort: low");
    assert_eq!(worker.command("/name widget spike").await, Some(true));
    assert_eq!(system_line(&mut ui).await, "session named widget spike");
    assert_eq!(worker.loop_.name().as_deref(), Some("widget spike"));
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}

#[tokio::test]
async fn bare_resume_reports_when_the_workspace_has_no_previous_session() {
    let (mut worker, _tx, mut ui, workspace) = worker_ui();

    assert_eq!(worker.command("/resume").await, Some(true));
    let failed = loop {
        match tokio::time::timeout(Duration::from_secs(5), ui.recv()).await {
            Ok(Some(Msg::Failed(text))) => break text,
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => panic!("no failure arrived"),
        }
    };

    assert_eq!(
        failed,
        "configuration error: no previous session for this workspace"
    );
    std::fs::remove_dir_all(workspace).unwrap_or_else(|error| panic!("{error}"));
}
