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
        fired_ceiling(limits.compact_chars, estimated, Some(window), None).is_none(),
        "character cap tripped a known window"
    );
}

#[test]
fn a_known_window_compacts_at_its_percent() {
    let limits = crate::limits::limits();
    let window = 500_000u32;
    let estimated = window as usize * limits.compact_percent / 100;
    assert!(fired_ceiling(0, estimated, Some(window), None).is_some());
    assert!(
        fired_ceiling(
            limits.compact_chars,
            estimated.saturating_sub(1),
            Some(window),
            None
        )
        .is_none()
    );
}

#[test]
fn an_unknown_window_compacts_at_the_character_cap() {
    let chars = crate::limits::limits().compact_chars;
    assert!(fired_ceiling(chars, chars / 4, None, None).is_some());
    assert!(fired_ceiling(chars - 1, (chars - 1) / 4, None, None).is_none());
}

#[test]
fn a_token_cap_does_not_replace_the_character_cap() {
    let chars = crate::limits::limits().compact_chars;
    assert!(fired_ceiling(chars, chars / 4, None, Some(1_000_000)).is_some());
    assert!(fired_ceiling(chars - 1, (chars - 1) / 4, None, Some(1_000_000)).is_none());
    assert!(fired_ceiling(0, 800_000, None, Some(1_000_000)).is_some());
}

#[test]
fn a_usage_sample_scales_the_whole_prompt() {
    assert_eq!(estimate(2_000, Some((1_000, 500))), 1_000);
    assert_eq!(estimate(100, Some((1_000, 500))), 50);
    assert_eq!(estimate(100, Some((0, 500))), 25);
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
    assert!(
        last.iter()
            .any(|message| message.content.contains("finish the kernel candidate"))
    );
    assert!(
        last.iter()
            .all(|message| !message.content.contains("Harness objective (verbatim")),
        "a normal turn restated an old objective"
    );
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

fn read_call(index: usize) -> crate::provider::scripted::Reply {
    use crate::provider::scripted::Reply;
    use crate::provider::{ToolCall, ToolFunction, ToolKind};
    Reply::Call(ToolCall {
        id: format!("t{index}"),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: serde_json::json!({"path": format!("f{index}.txt")}).to_string(),
        },
        continuation: None,
    })
}

async fn second_request(
    files: usize,
    file_body: &str,
    second_prompt: &str,
    driver_window: Option<u32>,
    role_window: Option<Option<u32>>,
    compact_tokens: Option<u32>,
) -> crate::Result<(String, String)> {
    use crate::provider::scripted::{Reply, Scripted};
    let workspace = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let mut replies = Vec::new();
    for index in 1..=files {
        std::fs::write(workspace.path().join(format!("f{index}.txt")), file_body)?;
        replies.push(read_call(index));
    }
    replies.push(Reply::Text("done"));
    replies.push(Reply::Text("ok"));
    let receiver = Scripted::new(replies);
    let sent = receiver.sent();
    let idle = Scripted::new(Vec::<Reply>::new());
    let session_driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system")?;
    let mut session = match role_window {
        Some(window) => {
            let mut session =
                super::super::Loop::open(idle, "m", workspace.path(), "system", session_driver)?;
            session.set_role_model(
                crate::workflow::Role::Implement,
                receiver,
                "selection",
                None,
                None,
                window,
            );
            session
        }
        None => {
            super::super::Loop::open(receiver, "m", workspace.path(), "system", session_driver)?
        }
    };
    session.set_context_window(driver_window);
    session.set_compact_tokens(compact_tokens);
    session.turn("read the files").await?;
    let second = session.turn(second_prompt).await?;
    let sent = sent.lock().expect("captured requests");
    let last = sent.last().expect("second request");
    let first = last
        .iter()
        .find(|message| message.tool_call_id.as_deref() == Some("t1"))
        .map(|message| message.content.clone())
        .unwrap_or_default();
    let note = second
        .events
        .iter()
        .filter_map(|event| match event {
            crate::tools::Event::Note { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok((first, note))
}

#[tokio::test]
async fn an_unknown_window_still_compacts_at_the_character_cap() -> crate::Result<()> {
    let body = format!("{}\n", "0123456789".repeat(8)).repeat(600);
    let (first, note) =
        second_request(14, &body, &"x".repeat(80_000), None, None, Some(1_000_000)).await?;
    assert!(
        first.starts_with("[cleared"),
        "token cap hid the character cap: {first}"
    );
    assert!(
        note.contains("prompt exceeded") && note.contains("tools"),
        "{note}"
    );
    Ok(())
}

#[tokio::test]
async fn compaction_uses_the_receiving_seats_window() -> crate::Result<()> {
    let (first, note) = second_request(
        4,
        &"0123456789".repeat(450),
        &"x".repeat(8_000),
        Some(1_000_000),
        Some(Some(8_000)),
        None,
    )
    .await?;
    assert!(
        first.starts_with("[cleared"),
        "driver window hid the selection window: {first}"
    );
    assert!(note.contains("at 80% of 8000"), "{note}");
    Ok(())
}

#[tokio::test]
async fn a_seated_window_compacts_without_a_driver_window() -> crate::Result<()> {
    let (first, note) = second_request(
        4,
        &"0123456789".repeat(450),
        &"x".repeat(8_000),
        None,
        Some(Some(8_000)),
        None,
    )
    .await?;
    assert!(
        first.starts_with("[cleared"),
        "unknown driver hid the selection window: {first}"
    );
    assert!(note.contains("at 80% of 8000"), "{note}");
    Ok(())
}

#[tokio::test]
async fn a_model_without_a_window_does_not_inherit_the_driver_window() -> crate::Result<()> {
    let (first, note) = second_request(
        4,
        &"0123456789".repeat(450),
        &"x".repeat(8_000),
        Some(8_000),
        Some(None),
        None,
    )
    .await?;
    assert!(
        !first.starts_with("[cleared"),
        "selection inherited the driver window: {first}"
    );
    assert!(note.is_empty(), "{note}");
    Ok(())
}

#[tokio::test]
async fn old_tool_outputs_clear_once_in_one_batch_before_the_summary_trigger() -> crate::Result<()>
{
    use crate::provider::scripted::{Reply, Scripted};
    let workspace = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let body = format!("{}\n", "0123456789".repeat(8)).repeat(190);
    let mut replies = Vec::new();
    for index in 1..=16 {
        std::fs::write(workspace.path().join(format!("f{index}.txt")), &body)?;
        replies.push(read_call(index));
    }
    replies.push(Reply::Text("done"));
    let provider = Scripted::new(replies);
    let sent = provider.sent();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system")?;
    let mut session = super::super::Loop::open(provider, "m", workspace.path(), "system", driver)?;
    session.set_context_window(Some(100_000));
    let turn = session.turn("read the files").await?;
    let clears = turn
        .events
        .iter()
        .filter(
            |event| matches!(event, crate::tools::Event::Note { text } if text.contains("cleared")),
        )
        .count();
    let sent = sent.lock().expect("captured requests");
    let last = sent.last().expect("final request");
    let output = |id: &str| {
        last.iter()
            .find(|message| message.tool_call_id.as_deref() == Some(id))
            .map(|message| message.content.clone())
            .unwrap_or_default()
    };
    assert_eq!(clears, 1);
    assert!(
        output("t1").starts_with("[cleared"),
        "{}",
        output("t1").len()
    );
    assert!(output("t16").contains("0123456789"));
    Ok(())
}
