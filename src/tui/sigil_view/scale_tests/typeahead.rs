use super::{State, busy_info, frame_with};

#[test]
fn commands_suggest_and_complete_while_a_turn_runs() {
    let mut completed = String::new();
    let shown = frame_with(busy_info(), 110, 30, State::Busy, |app| {
        app.replace("/sett".to_owned());
        assert!(app.complete());
        completed = app.draft.peek();
        app.replace("/sett".to_owned());
    })
    .join("\n");
    assert!(completed.starts_with("/settings"), "{completed:?}");
    assert!(
        shown.contains("commands") && shown.contains("/settings"),
        "{shown}"
    );
}

#[test]
fn enter_queues_a_message_mid_turn_and_an_immediate_enter_sends_it() {
    use super::super::app::App;
    use crate::tui::{Activity, Cmd, Msg};
    sigil::reactive::Runtime::scope("queue-then-send", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.replace("also check the tests".to_owned());
        app.submit();
        app.submit();
        let sent: Vec<Cmd> = std::iter::from_fn(|| sent.try_recv().ok()).collect();
        assert!(
            matches!(sent.as_slice(), [.., Cmd::Queue(input), Cmd::SteerQueued] if input.text == "also check the tests"),
            "{} commands sent",
            sent.len()
        );
    });
}

#[test]
fn enter_on_an_empty_composer_never_answers_a_pending_approval() {
    use super::super::app::App;
    use crate::tui::{Activity, Cmd, Msg};
    sigil::reactive::Runtime::scope("queue-approval", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.replace("also check the tests".to_owned());
        app.submit();
        app.update(Msg::Queued {
            steering: 0,
            queued: 1,
        });
        app.update(Msg::Event(crate::Event::Approval {
            detail: Some("rm -rf build".to_owned()),
            persist: false,
        }));
        app.submit();
        let steered = std::iter::from_fn(|| sent.try_recv().ok())
            .filter(|command| matches!(command, Cmd::SteerQueued))
            .count();
        assert_eq!(steered, 0);
    });
}

#[test]
fn messages_sent_into_the_turn_no_longer_count_as_queued() {
    use crate::tui::Msg;
    let shown = frame_with(busy_info(), 110, 30, State::Busy, |app| {
        app.update(Msg::Queued {
            steering: 1,
            queued: 0,
        });
    })
    .join("\n");
    assert!(!shown.contains("queued"), "{shown}");
}
