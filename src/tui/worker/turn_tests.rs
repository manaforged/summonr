use crate::tui::Fact;
use std::collections::VecDeque;

use super::super::steer::{InputQueue, steerable};
use super::{Cmd, Msg};
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
}

impl Fixture {
    fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let (ui, inbox) = tokio::sync::mpsc::unbounded_channel();
        Self {
            workspace,
            ui,
            inbox,
            steering: std::sync::Arc::default(),
        }
    }

    fn queue(&self, deferred: VecDeque<Cmd>, followups: VecDeque<UserInput>) -> InputQueue {
        let mut queue = InputQueue::new(self.steering.clone());
        queue.deferred = deferred;
        queue.followups = followups;
        queue
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
    let fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let mut queue = fixture.queue(VecDeque::new(), VecDeque::new());
    for line in ["check the tests", "!ls", "and the logs"] {
        queue.during_turn(Cmd::Steer(UserInput::from(line)), &path, &fixture.ui);
    }
    assert_eq!(fixture.steered(), ["check the tests"]);
    assert_eq!(texts(&queue.followups), ["!ls", "and the logs"]);
}

#[test]
fn steering_queue_count_keeps_messages_held_behind_a_deferred_command() {
    let mut fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let mut queue = fixture.queue(VecDeque::new(), VecDeque::new());
    queue.during_turn(
        Cmd::Queue(UserInput::from("check the tests")),
        &path,
        &fixture.ui,
    );
    queue.defer(Cmd::Command("/model opus".to_string()), &fixture.ui);
    let last = fixture
        .messages()
        .into_iter()
        .filter_map(|msg| match msg {
            Msg::Fact(Fact::Queued { queued, .. }) => Some(queued),
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
    let mut queue = fixture.queue(std::mem::take(deferred), std::mem::take(followups));
    queue.during_turn(Cmd::SteerQueued, &path, &fixture.ui);
    *deferred = queue.deferred;
    *followups = queue.followups;
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
        Msg::Fact(Fact::System(text)) if text.contains("after the turn")
    )));
}

#[test]
fn steering_queue_runs_an_effort_change_after_messages_queued_before_it() {
    let fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let mut queue = fixture.queue(VecDeque::new(), VecDeque::new());
    queue.during_turn(Cmd::Queue(UserInput::from("!check")), &path, &fixture.ui);
    queue.defer(
        Cmd::SetEffort(Some(crate::provider::Effort::High)),
        &fixture.ui,
    );
    let deferred = queue.deferred;
    assert!(matches!(
        deferred.front(),
        Some(Cmd::Submit(input)) if input.text == "!check"
    ));
    assert!(matches!(deferred.back(), Some(Cmd::SetEffort(_))));
}

fn worker() -> (
    super::Worker,
    tokio::sync::mpsc::UnboundedReceiver<Msg>,
    [tempfile::TempDir; 2],
) {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let client = crate::provider::Client::new("test", "http://127.0.0.1:9/v1").unwrap();
    let driver = crate::durable::driver::SessionDriver::open(
        home.path(),
        workspace.path(),
        "gpt-5.4",
        "system",
        None,
    )
    .unwrap();
    let loop_ =
        crate::runtime::Loop::open(client, "gpt-5.4", workspace.path(), "system", driver).unwrap();
    let info = super::super::Info {
        version: String::new(),
        model: "gpt-5.4".to_string(),
        relay: None,
        session: loop_.session_id(),
        endpoint: "openai".to_string(),
        pick: None,
        prefs: crate::prefs::Prefs::default(),
        splash: false,
        inline: false,
        strict: false,
        workspace: workspace.path().display().to_string(),
        workflow: None,
        config: None,
        prompt: None,
    };
    let (_commands, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (ui, inbox) = tokio::sync::mpsc::unbounded_channel();
    (
        super::Worker::new(loop_, receiver, ui, info),
        inbox,
        [home, workspace],
    )
}

#[tokio::test]
async fn escape_restores_a_deferred_submit_and_keeps_the_after_turn_command() {
    let (mut worker, mut inbox, _dirs) = worker();
    worker.queue.deferred = VecDeque::from([
        Cmd::Submit(UserInput::from("first")),
        Cmd::Command("/model opus".to_string()),
    ]);
    worker.queue.followups = VecDeque::from([UserInput::from("second")]);
    worker.recover();
    let restored = std::iter::from_fn(|| inbox.try_recv().ok())
        .find_map(|msg| match msg {
            Msg::Fact(Fact::Restored(inputs)) => Some(inputs),
            _other => None,
        })
        .expect("restored draft");
    let restored = restored
        .iter()
        .map(|input| input.text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(restored, ["first", "second"]);
    assert!(matches!(
        worker.queue.deferred.iter().collect::<Vec<_>>().as_slice(),
        [Cmd::Command(line)] if line == "/model opus"
    ));
}

#[test]
fn dequeue_clears_the_recorded_text_of_what_it_takes() {
    let fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let mut queue = fixture.queue(VecDeque::new(), VecDeque::new());
    queue.during_turn(Cmd::Submit(UserInput::from("again")), &path, &fixture.ui);
    assert_eq!(queue.recorded, ["again"]);
    queue.during_turn(Cmd::Dequeue, &path, &fixture.ui);
    assert!(queue.recorded.is_empty());
}

#[test]
fn identical_pending_rows_follow_the_steering_queue() {
    let mut fixture = Fixture::new();
    let path = fixture.workspace.path().to_path_buf();
    let mut queue = fixture.queue(VecDeque::new(), VecDeque::new());
    for _ in 0..2 {
        queue.during_turn(Cmd::Steer(UserInput::from("same")), &path, &fixture.ui);
    }
    fixture.steering.lock().unwrap().pop_front();
    queue.announce(&fixture.ui);
    let rows = fixture
        .messages()
        .into_iter()
        .filter_map(|msg| match msg {
            Msg::Fact(Fact::Pending(rows)) => Some(rows),
            _other => None,
        })
        .last();
    assert_eq!(rows, Some(vec![(true, "same".to_string())]));
}
