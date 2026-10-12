use std::collections::VecDeque;

use super::{Cmd, MidTurn, Msg, steerable};
use crate::provider::UserInput;

#[test]
fn steering_queue_sends_plain_messages_and_holds_shell_slash_and_image_lines() {
    for line in [
        "!git status",
        "!!cargo test",
        "/review the diff",
        "/github:pr 12",
    ] {
        assert!(!steerable(&UserInput::from(line)), "{line}");
    }
    for line in [
        "also check the tests",
        "  why did that fail?",
        "it says ! here",
    ] {
        assert!(steerable(&UserInput::from(line)), "{line}");
    }
    let mut input = UserInput::from("look at this");
    input
        .images
        .push(crate::provider::Image::encode("image/png", &[1, 2, 3]));
    assert!(!steerable(&input));
}

struct Fixture {
    workspace: tempfile::TempDir,
    ui: tokio::sync::mpsc::UnboundedSender<Msg>,
    inbox: tokio::sync::mpsc::UnboundedReceiver<Msg>,
    steering: std::sync::Arc<crate::runtime::Steering>,
    reader: super::super::reader::Reader,
    auto_reply: bool,
}

impl Fixture {
    fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let (ui, inbox) = tokio::sync::mpsc::unbounded_channel();
        let reader = super::super::reader::Reader::new(
            workspace.path().to_path_buf(),
            Default::default(),
            ui.clone(),
        );
        Self {
            workspace,
            ui,
            inbox,
            steering: std::sync::Arc::default(),
            reader,
            auto_reply: false,
        }
    }

    fn mid<'a>(
        &'a mut self,
        deferred: &'a mut VecDeque<Cmd>,
        followups: &'a mut VecDeque<UserInput>,
    ) -> MidTurn<'a> {
        MidTurn {
            deferred,
            followups,
            steering: &self.steering,
            auto_reply: &mut self.auto_reply,
            ui: &self.ui,
            reader: &self.reader,
        }
    }

    fn steered(&self) -> Vec<String> {
        let queue = self.steering.lock().unwrap();
        queue.iter().map(|input| input.text.clone()).collect()
    }

    fn messages(&mut self) -> Vec<Msg> {
        std::iter::from_fn(|| self.inbox.try_recv().ok()).collect()
    }
}

fn texts(inputs: &VecDeque<UserInput>) -> Vec<&str> {
    inputs.iter().map(|input| input.text.as_str()).collect()
}

#[test]
fn steering_queue_routes_plain_lines_into_the_turn_and_keeps_the_rest_in_order() {
    let mut fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let (mut deferred, mut followups) = (VecDeque::new(), VecDeque::new());
    let mut mid = fixture.mid(&mut deferred, &mut followups);
    for line in ["check the tests", "!ls", "and the logs"] {
        mid.queued(Cmd::Steer(UserInput::from(line)), &path);
    }
    assert_eq!(fixture.steered(), ["check the tests"]);
    assert_eq!(texts(&followups), ["!ls", "and the logs"]);
}

#[test]
fn steering_queue_count_keeps_messages_held_behind_a_deferred_command() {
    let mut fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let (mut deferred, mut followups) = (VecDeque::new(), VecDeque::new());
    let mut mid = fixture.mid(&mut deferred, &mut followups);
    mid.queued(Cmd::Queue(UserInput::from("check the tests")), &path);
    mid.later(Cmd::Command("/model opus".to_string()));
    let last = fixture
        .messages()
        .into_iter()
        .filter_map(|msg| match msg {
            Msg::Queued { queued, .. } => Some(queued),
            _other => None,
        })
        .last();
    assert_eq!(last, Some(1));
}

fn blank_enter(
    deferred: &mut VecDeque<Cmd>,
    followups: &mut VecDeque<UserInput>,
) -> (Vec<String>, Vec<Msg>) {
    let mut fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    fixture
        .mid(deferred, followups)
        .queued(Cmd::SteerQueued, &path);
    (fixture.steered(), fixture.messages())
}

#[test]
fn blank_enter_sends_plain_messages_held_behind_a_deferred_command_or_a_shell_line() {
    let mut deferred = VecDeque::from([
        Cmd::Submit(UserInput::from("check the logs")),
        Cmd::Command("/model opus".to_string()),
    ]);
    let mut followups = VecDeque::from([UserInput::from("!ls"), UserInput::from("and the tests")]);
    let (steered, _) = blank_enter(&mut deferred, &mut followups);
    assert_eq!(steered, ["check the logs", "and the tests"]);
    assert!(matches!(
        deferred.iter().collect::<Vec<_>>().as_slice(),
        [Cmd::Command(line)] if line == "/model opus"
    ));
    assert_eq!(texts(&followups), ["!ls"]);
}

#[test]
fn blank_enter_says_why_when_only_shell_and_slash_lines_wait() {
    let mut deferred = VecDeque::new();
    let mut followups = VecDeque::from([UserInput::from("!ls")]);
    let (steered, notes) = blank_enter(&mut deferred, &mut followups);
    assert!(steered.is_empty());
    assert!(notes.iter().any(|msg| matches!(
        msg,
        Msg::System(text) if text.contains("after the turn")
    )));
}

#[tokio::test]
async fn steering_queue_runs_an_effort_change_after_messages_queued_before_it() {
    let mut fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let (mut deferred, mut followups) = (VecDeque::new(), VecDeque::new());
    let mut mid = fixture.mid(&mut deferred, &mut followups);
    mid.queued(Cmd::Queue(UserInput::from("!check")), &path);
    mid.command(
        Cmd::SetEffort(Some(crate::provider::Effort::High)),
        &path,
        "session",
        &path,
    )
    .await;
    assert!(matches!(
        deferred.front(),
        Some(Cmd::Submit(input)) if input.text == "!check"
    ));
    assert!(matches!(deferred.back(), Some(Cmd::SetEffort(_))));
}
