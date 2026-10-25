use super::*;

#[test]
fn a_known_window_does_not_compact_at_the_character_cap() {
    let limits = crate::limits::limits();
    let window = 500_000u32;
    let estimated = limits.compact_chars / 3;
    assert!(
        estimated.saturating_mul(100) < window as usize * limits.compact_percent,
        "fixture is already over the window percent"
    );
    assert!(
        !prompt_pressure(limits.compact_chars, estimated, Some(window)),
        "character cap tripped a known window"
    );
}

#[test]
fn a_known_window_compacts_at_its_percent() {
    let limits = crate::limits::limits();
    let window = 500_000u32;
    let estimated = window as usize * limits.compact_percent / 100;
    assert!(prompt_pressure(0, estimated, Some(window)));
    assert!(!prompt_pressure(
        limits.compact_chars,
        estimated.saturating_sub(1),
        Some(window)
    ));
}

#[test]
fn an_unknown_window_compacts_at_the_character_cap() {
    let chars = crate::limits::limits().compact_chars;
    assert!(prompt_pressure(chars, chars / 4, None));
    assert!(!prompt_pressure(chars - 1, (chars - 1) / 4, None));
}

#[test]
fn a_known_window_elides_to_its_percent_not_the_character_floor() {
    let limits = crate::limits::limits();
    let window = 500_000u32;
    let estimated = window as usize * limits.compact_percent / 100;
    let chars = estimated * 3;
    let target = elide_target(chars, estimated, Some(window));
    let floor = limits.compact_chars * limits.elide_percent / limits.compact_percent.max(1);
    assert!(
        target > floor,
        "elide target {target} stayed on the character floor {floor}"
    );
    assert_eq!(
        target,
        chars * (window as usize * limits.elide_percent / 100) / estimated
    );
}

#[tokio::test]
async fn a_known_window_keeps_output_under_its_percent() -> crate::Result<()> {
    use crate::provider::scripted::{Reply, Scripted};
    use crate::provider::{ToolCall, ToolFunction, ToolKind};
    let workspace = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let mut replies = Vec::new();
    for index in 1..=24 {
        let name = format!("source{index}.txt");
        std::fs::write(
            workspace.path().join(&name),
            format!("{}\n", "0123456789".repeat(8)).repeat(600),
        )?;
        replies.push(Reply::Call(ToolCall {
            id: format!("t{index}"),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
                name: "read".to_string(),
                arguments: serde_json::json!({"path": name, "limit": 800}).to_string(),
            },
            continuation: None,
        }));
    }
    replies.push(Reply::Text("candidate ready"));
    let provider = Scripted::new(replies);
    let sent = provider.sent();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system")?;
    let mut session = super::super::Loop::open(provider, "m", workspace.path(), "system", driver)?;
    session.set_context_window(Some(1_000_000));
    session.turn("finish the kernel candidate").await?;
    let sent = sent.lock().expect("captured provider requests");
    let last = sent.last().expect("final provider request");
    let first = last
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("t1"))
        .expect("first output");
    assert!(
        !first.content.starts_with("[cleared"),
        "cleared at {} chars, under the window percent",
        first.content.len()
    );
    assert!(last.iter().any(
        |message| message.content.contains("Harness objective (verbatim")
            && message.content.contains("finish the kernel candidate")
    ));
    assert!(
        last.iter()
            .any(|message| message.tool_call_id.as_deref() == Some("t24")
                && message.content.contains("0123456789"))
    );
    Ok(())
}

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

#[tokio::test]
async fn compaction_clears_old_tool_output_before_it_asks_for_a_summary() {
    use crate::provider::scripted::{Reply, Scripted};
    use crate::provider::{ToolCall, ToolFunction, ToolKind};
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let line = "0123456789012345678901234567890123456789\n";
    for index in 1..=8 {
        std::fs::write(
            workspace.path().join(format!("big{index}.txt")),
            line.repeat(600),
        )
        .unwrap();
    }
    let read = |id: &str| {
        Reply::Call(ToolCall {
            id: id.to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
                name: "read".to_string(),
                arguments: serde_json::json!({ "path": format!("big{}.txt", &id[1..]) })
                    .to_string(),
            },
            continuation: None,
        })
    };
    let provider = Scripted::new(
        (1..=8)
            .map(|index| read(&format!("t{index}")))
            .chain([Reply::Text("done")]),
    );
    let sent = provider.sent();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system").unwrap();
    let mut session =
        super::super::Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    session.set_context_window(Some(30_000));
    let turn = session.turn("read the eight files").await.unwrap();
    assert_eq!(turn.text, "done");
    let sent = sent.lock().unwrap();
    assert!(
        sent.iter()
            .flatten()
            .all(|message| !message.content.contains("Summarize this session")),
        "a summary was requested"
    );
    let last = sent.last().unwrap();
    let output = |id: &str| {
        last.iter()
            .find(|message| message.tool_call_id.as_deref() == Some(id))
            .map(|message| message.content.clone())
            .unwrap_or_default()
    };
    assert!(output("t1").starts_with("[cleared"), "{}", output("t1"));
    assert!(output("t8").contains(line), "the newest output was cleared");
}
