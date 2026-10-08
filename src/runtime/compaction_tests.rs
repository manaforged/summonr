use super::*;

#[test]
fn the_kept_tail_never_starts_before_the_turn_opener() {
    let message = |role: Role| Message {
        role,
        ..Message::system("0123456789")
    };
    let messages = [
        Message::system("0123456789"),
        message(Role::User),
        message(Role::Assistant),
        message(Role::User),
        message(Role::Assistant),
        message(Role::Assistant),
    ];
    assert_eq!(compact_boundary(&messages, 40, false, Some(2)), Some(4));
}

#[tokio::test]
async fn a_compaction_summary_reuses_the_session_prefix_and_history() {
    use crate::provider::scripted::{Reply, Scripted};
    use crate::provider::{ToolCall, ToolFunction, ToolKind};
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("a.txt"), "one\n").unwrap();
    let read = Reply::Call(ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: serde_json::json!({ "path": "a.txt" }).to_string(),
        },
        continuation: None,
    });
    let provider = Scripted::new([
        read,
        Reply::Text("it says one"),
        Reply::Text("ok"),
        Reply::Text("summary"),
    ]);
    let sent = provider.sent();
    let prefixes = provider.prefixes();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system").unwrap();
    let mut session =
        super::super::Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    session.turn("read a.txt").await.unwrap();
    session.turn("again").await.unwrap();
    assert!(session.compact().await.unwrap());
    let prefixes = prefixes.lock().unwrap();
    assert_eq!(prefixes.len(), 4);
    assert!(
        prefixes.iter().all(|hash| *hash == prefixes[0]),
        "{prefixes:?}"
    );
    let sent = sent.lock().unwrap();
    let summary = sent.last().unwrap();
    assert!(
        summary
            .iter()
            .any(|message| message.role == Role::Tool
                && message.tool_call_id.as_deref() == Some("t1")),
        "{summary:?}"
    );
    assert_eq!(summary[..3], sent[1][..3]);
}
