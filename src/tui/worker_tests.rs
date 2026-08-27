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

#[test]
fn model_setting_reuses_the_current_endpoint() {
    let (mut worker, _tx, workspace) = worker();
    worker.endpoint = "qwen".to_string();

    worker.set_model("qwen-next");

    assert_eq!(worker.loop_.model(), "qwen-next");
    assert_eq!(worker.endpoint, "qwen");
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
