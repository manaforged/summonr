use super::steerable;
use crate::provider::UserInput;

#[test]
fn steering_queue_holds_shell_and_slash_lines_for_their_own_turn() {
    for line in [
        "!git status",
        "!!cargo test",
        "/review the diff",
        "/github:pr 12",
    ] {
        assert!(!steerable(&UserInput::from(line)), "{line}");
    }
}

#[test]
fn steering_queue_sends_plain_messages_and_bang_inside_text() {
    for line in [
        "also check the tests",
        "  why did that fail?",
        "it says ! here",
    ] {
        assert!(steerable(&UserInput::from(line)), "{line}");
    }
}

#[test]
fn steering_queue_holds_images_for_the_turn_that_checks_them() {
    let mut input = UserInput::from("look at this");
    input
        .images
        .push(crate::provider::Image::encode("image/png", &[1, 2, 3]));
    assert!(!steerable(&input));
}

fn mid_turn_fixture() -> (
    tempfile::TempDir,
    tokio::sync::mpsc::UnboundedSender<super::Msg>,
    tokio::sync::mpsc::UnboundedReceiver<super::Msg>,
    std::sync::Arc<crate::runtime::Steering>,
) {
    let workspace = tempfile::tempdir().unwrap();
    let (ui, inbox) = tokio::sync::mpsc::unbounded_channel();
    (workspace, ui, inbox, std::sync::Arc::default())
}

#[test]
fn steering_queue_routes_plain_lines_into_the_turn_and_keeps_the_rest_in_order() {
    let (workspace, ui, _inbox, steering) = mid_turn_fixture();
    let reader = super::super::reader::Reader::new(
        workspace.path().to_path_buf(),
        Default::default(),
        ui.clone(),
    );
    let (mut deferred, mut followups, mut auto_reply) =
        (Default::default(), Default::default(), false);
    let mut mid = super::MidTurn {
        deferred: &mut deferred,
        followups: &mut followups,
        steering: &steering,
        auto_reply: &mut auto_reply,
        ui: &ui,
        reader: &reader,
    };
    for line in ["check the tests", "!ls", "and the logs"] {
        mid.queued(super::Cmd::Steer(UserInput::from(line)), workspace.path());
    }
    let steered: Vec<String> = steering
        .lock()
        .unwrap()
        .iter()
        .map(|input| input.text.clone())
        .collect();
    assert_eq!(steered, ["check the tests"]);
    let waiting: Vec<&str> = followups.iter().map(|input| input.text.as_str()).collect();
    assert_eq!(waiting, ["!ls", "and the logs"]);
}

#[test]
fn steering_queue_count_keeps_messages_held_behind_a_deferred_command() {
    let (workspace, ui, mut inbox, steering) = mid_turn_fixture();
    let reader = super::super::reader::Reader::new(
        workspace.path().to_path_buf(),
        Default::default(),
        ui.clone(),
    );
    let (mut deferred, mut followups, mut auto_reply) =
        (Default::default(), Default::default(), false);
    let mut mid = super::MidTurn {
        deferred: &mut deferred,
        followups: &mut followups,
        steering: &steering,
        auto_reply: &mut auto_reply,
        ui: &ui,
        reader: &reader,
    };
    mid.queued(
        super::Cmd::Queue(UserInput::from("check the tests")),
        workspace.path(),
    );
    mid.later(super::Cmd::Command("/model opus".to_string()));
    let last = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|msg| match msg {
            super::Msg::Queued { queued, .. } => Some(queued),
            _other => None,
        })
        .last();
    assert_eq!(last, Some(1));
}

#[test]
fn steering_queue_keeps_held_messages_behind_a_deferred_command_on_blank_enter() {
    let (workspace, ui, _inbox, steering) = mid_turn_fixture();
    let reader = super::super::reader::Reader::new(
        workspace.path().to_path_buf(),
        Default::default(),
        ui.clone(),
    );
    let (mut deferred, mut followups, mut auto_reply) =
        (Default::default(), Default::default(), false);
    let mut mid = super::MidTurn {
        deferred: &mut deferred,
        followups: &mut followups,
        steering: &steering,
        auto_reply: &mut auto_reply,
        ui: &ui,
        reader: &reader,
    };
    mid.later(super::Cmd::Command("/model opus".to_string()));
    mid.queued(
        super::Cmd::Steer(UserInput::from("continue after setup")),
        workspace.path(),
    );
    mid.queued(super::Cmd::SteerQueued, workspace.path());
    assert!(steering.lock().unwrap().is_empty());
    let waiting: Vec<&str> = followups.iter().map(|input| input.text.as_str()).collect();
    assert_eq!(waiting, ["continue after setup"]);
}

#[tokio::test]
async fn steering_queue_runs_an_effort_change_after_messages_queued_before_it() {
    let (workspace, ui, _inbox, steering) = mid_turn_fixture();
    let reader = super::super::reader::Reader::new(
        workspace.path().to_path_buf(),
        Default::default(),
        ui.clone(),
    );
    let (mut deferred, mut followups, mut auto_reply) =
        (Default::default(), Default::default(), false);
    let mut mid = super::MidTurn {
        deferred: &mut deferred,
        followups: &mut followups,
        steering: &steering,
        auto_reply: &mut auto_reply,
        ui: &ui,
        reader: &reader,
    };
    mid.queued(
        super::Cmd::Queue(UserInput::from("!check")),
        workspace.path(),
    );
    mid.command(
        super::Cmd::SetEffort(Some(crate::provider::Effort::High)),
        workspace.path(),
        "session",
        workspace.path(),
    )
    .await;
    assert!(matches!(
        deferred.front(),
        Some(super::Cmd::Submit(input)) if input.text == "!check"
    ));
    assert!(matches!(deferred.back(), Some(super::Cmd::SetEffort(_))));
}
