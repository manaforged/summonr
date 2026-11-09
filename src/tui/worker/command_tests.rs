use std::sync::Arc;
use std::time::Duration;

use super::*;

#[tokio::test]
async fn input_typed_while_a_command_runs_reports_the_waiting_steer() {
    let dir = tempfile::tempdir().unwrap();
    let (ui, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), Default::default(), ui.clone());
    let steering: Arc<crate::runtime::Steering> = Arc::default();
    steering
        .lock()
        .unwrap()
        .push_back(UserInput::from("check the logs"));
    let mut queue = InputQueue::new(steering);
    let (send, mut cmd) = tokio::sync::mpsc::unbounded_channel();
    send.send(Cmd::Queue(UserInput::from("then run the tests")))
        .unwrap();
    let outcome = wait_cancelable(
        &mut cmd,
        &mut queue,
        &reader,
        &ui,
        None,
        tokio::time::sleep(Duration::from_millis(200)),
    )
    .await;
    assert!(matches!(outcome, Completion::Done(())));
    let counts: Vec<(usize, usize)> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|message| match message {
            Msg::Queued { steering, queued } => Some((steering, queued)),
            _other => None,
        })
        .collect();
    assert_eq!(counts, [(1, 1)]);
}
