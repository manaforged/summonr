use serde_json::json;

use crate::support::http::{Mode, Remote};
use crate::support::{Model, Session, Step, calls};

pub(crate) fn a_call_with_no_reply_is_cancelled_at_the_timeout() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__hang", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", "timeout_secs = 1"), &[]);
    assert!(
        model.tool_results()[0].contains("no reply in 1s"),
        "{:?}",
        model.tool_results()
    );
    let received = session.received("fixture1");
    let call = calls(&received, "tools/call")[0]["id"].clone();
    let cancelled = calls(&received, "notifications/cancelled");
    assert_eq!(cancelled.len(), 1, "{received:?}");
    assert_eq!(cancelled[0]["params"]["requestId"], call);
}

pub(crate) fn a_timed_out_reply_does_not_break_the_next_call() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call(
            "fixture1__split",
            json!({"bytes": 10 * 1024 * 1024, "ms": 3000}),
        ),
        Step::Call("fixture1__echo", json!({"text": "after"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", "timeout_secs = 2"), &[]);
    let results = model.tool_results();
    assert!(results[0].contains("no reply in 2s"), "{results:?}");
    assert!(results[1].contains("after"), "{results:?}");
}

pub(crate) fn a_request_the_server_does_not_read_times_out() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__deaf", json!({})),
        Step::Call("fixture1__echo", json!({"text": "x".repeat(300_000)})),
        Step::Call("fixture1__echo", json!({"text": "after"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", "timeout_secs = 1"), &[]);
    let results = model.tool_results();
    assert!(
        results[1].contains("no reply in 1s"),
        "{}",
        results[1].chars().take(200).collect::<String>()
    );
    assert!(
        results[2].contains("after"),
        "{}",
        results[2].chars().take(200).collect::<String>()
    );
}

pub(crate) fn a_remote_server_reconnects_after_its_session_expires() {
    let session = Session::new();
    let remote = Remote::start(Mode::Expire);
    let model = Model::start(vec![
        Step::Call("remote__echo", json!({})),
        Step::Call("remote__echo", json!({})),
        Step::Call("remote__echo", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &remote.config("remote", ""), &[]);
    let results = model.tool_results();
    assert!(results[0].contains("remote x"), "{results:?}");
    assert!(results[1].contains("404"), "{results:?}");
    assert!(results[2].contains("remote x"), "{results:?}");
}

pub(crate) fn a_remote_reply_on_a_stream_held_open_returns_at_once() {
    let session = Session::new();
    let remote = Remote::start(Mode::HeldOpen);
    let model = Model::start(vec![
        Step::Call("remote__echo", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &remote.config("remote", "timeout_secs = 5"), &[]);
    assert!(
        model.tool_results()[0].contains("remote x"),
        "{:?}",
        model.tool_results()
    );
}
