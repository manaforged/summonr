use std::path::Path;

use crate::durable::id::SessionId;
use crate::durable::log::{self, Line, Log, State, Usage};
use crate::provider::{Image, Message, Role, ToolCall, ToolFunction, ToolKind};

fn header(id: u128) -> Line {
    Line::Session {
        id: SessionId::from_u128(id),
        model: "test-model".to_string(),
        system: "You are Mage.".to_string(),
        workspace: Path::new("/tmp/mage").to_path_buf(),
        parent: None,
        depth: 0,
    }
}

fn usage(input: u64, output: u64, millis: u64) -> Usage {
    Usage {
        input,
        output,
        cached: 0,
        millis,
    }
}

fn user(text: &str, start: bool) -> Line {
    Line::User {
        text: text.to_string(),
        source: None,
        images: Vec::new(),
        start,
    }
}

fn assistant(text: &str, calls: &[&str]) -> Line {
    Line::Assistant {
        text: text.to_string(),
        reasoning: "thinking".to_string(),
        tool_calls: calls
            .iter()
            .map(|id| ToolCall {
                continuation: None,
                id: (*id).to_string(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"a"}"#.to_string(),
                },
            })
            .collect(),
        usage: usage(10, 5, 100),
        model: "test-model".to_string(),
    }
}

fn fold(lines: Vec<Line>) -> State {
    let mut state = State::default();
    for line in lines {
        state.apply(line);
    }
    state
}

#[test]
fn every_line_round_trips_through_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = log::session_path(dir.path(), SessionId::from_u128(7));
    let lines = vec![
        header(7),
        Line::Budget {
            tokens: Some(1_000),
            millis: None,
        },
        Line::User {
            text: "look".to_string(),
            source: None,
            images: vec![Image {
                media_type: "image/png".to_string(),
                bytes: b"png-bytes".to_vec(),
            }],
            start: true,
        },
        assistant("reading", &["c1"]),
        Line::Tool {
            call: "c1".to_string(),
            text: "contents".to_string(),
            success: true,
            millis: 4,
        },
        Line::Charge {
            usage: usage(1, 2, 3),
            reported: true,
            note: "goal judge".to_string(),
            model: "judge-model".to_string(),
        },
        Line::Workflow {
            name: Some("cheap".to_string()),
        },
        Line::Receipt {
            text: "done\n\nTurn receipt".to_string(),
        },
        Line::Summary {
            text: "summary".to_string(),
            tail: Vec::new(),
        },
        Line::Clear,
        Line::Close { reason: None },
        Line::Close {
            reason: Some("cancelled".to_string()),
        },
    ];
    let mut log = Log::create(&path).expect("create");
    for line in &lines {
        log.append(line).expect("append");
    }
    drop(log);

    assert_eq!(log::read(&path).expect("read"), lines);
    assert_eq!(log::parse_path(&path), Some(SessionId::from_u128(7)));
    assert_eq!(log::home_of(&path), Some(dir.path()));
    assert!(matches!(
        log::header(&path).expect("header"),
        Line::Session { .. }
    ));
    assert!(
        Log::create(&path).is_err(),
        "create must not clobber a session"
    );
}

#[test]
fn a_torn_last_line_is_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("session.jsonl");
    let mut text = serde_json::to_string(&header(1)).expect("encode");
    text.push('\n');
    text.push_str(r#"{"kind":"user","text":"half"#);
    std::fs::write(&path, text).expect("write");

    let lines = log::read(&path).expect("read");
    assert_eq!(lines.len(), 1);
    assert!(matches!(lines[0], Line::Session { .. }));
}

#[test]
fn state_folds_messages_usage_and_the_open_turn() {
    let state = fold(vec![
        header(1),
        Line::Budget {
            tokens: Some(50),
            millis: Some(60),
        },
        user("read a", true),
        assistant("reading", &["c1", "c2"]),
        Line::Tool {
            call: "c1".to_string(),
            text: "A".to_string(),
            success: true,
            millis: 7,
        },
    ]);
    assert_eq!(state.id, Some(SessionId::from_u128(1)));
    assert_eq!((state.tokens, state.millis), (Some(50), Some(60)));
    assert_eq!(state.spent, 15);
    assert_eq!(state.elapsed, 107);
    assert_eq!(state.opener, Some(0));
    assert_eq!(state.unanswered, vec!["c2".to_string()]);
    let history = &state.history;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, Role::User);
    assert_eq!(history[1].role, Role::Assistant);
    assert_eq!(history[1].reasoning.as_deref(), Some("thinking"));
    assert_eq!(history[1].tool_calls.len(), 2);
    assert_eq!(history[2].role, Role::Tool);
    assert_eq!(history[2].tool_call_id.as_deref(), Some("c1"));

    let replay = state.replay();
    assert_eq!(replay.model.as_deref(), Some("test-model"));
    assert_eq!(replay.spent_tokens, 15);
    assert_eq!(replay.history, state.history);
}

#[test]
fn receipt_preserves_the_assistant_and_adds_harness_feedback() {
    let state = fold(vec![
        header(1),
        user("write", true),
        assistant("done", &[]),
        Line::Receipt {
            text: "done\n\nTurn receipt".to_string(),
        },
        Line::Close { reason: None },
    ]);
    assert_eq!(state.history[1].content, "done");
    assert_eq!(state.history[1].role, Role::Assistant);
    assert_eq!(state.history[2].content, "done\n\nTurn receipt");
    assert_eq!(state.history[2].role, Role::System);
    assert_eq!(state.opener, None);
}

#[test]
fn summary_keeps_the_open_turn_opener_and_clear_drops_everything() {
    let mut state = fold(vec![
        header(1),
        user("old", true),
        assistant("old answer", &[]),
        Line::Close { reason: None },
        user("current", true),
        assistant("long answer", &[]),
        Line::Summary {
            text: "Keep the short path.".to_string(),
            tail: Vec::new(),
        },
    ]);
    assert_eq!(state.history.len(), 2);
    assert_eq!(state.history[0].content, "current");
    assert_eq!(state.history[1].role, Role::System);
    assert_eq!(state.history[1].content, "Keep the short path.");
    assert_eq!(state.opener, Some(0));

    state.apply(Line::Close { reason: None });
    state.apply(Line::Summary {
        text: "idle summary".to_string(),
        tail: Vec::new(),
    });
    assert_eq!(state.history.len(), 1);
    assert_eq!(state.history[0].role, Role::System);

    state.apply(Line::Clear);
    assert!(state.history.is_empty());
}

#[test]
fn charge_counts_without_a_message() {
    let state = fold(vec![
        header(1),
        user("go", true),
        Line::Charge {
            usage: usage(70, 30, 25),
            reported: false,
            note: "denied".to_string(),
            model: String::new(),
        },
    ]);
    assert_eq!(state.spent, 100);
    assert_eq!(state.unreported, 100);
    assert_eq!(state.elapsed, 25);
    assert_eq!(state.history.len(), 1);
}

#[test]
fn session_ids_are_32_hex_digits() {
    let id = SessionId::from_u128(0xabc);
    let text = id.to_string();
    assert_eq!(text.len(), 32);
    assert_eq!(text.parse::<SessionId>().expect("parse"), id);
    "abc".parse::<SessionId>().unwrap_err();
    "g".repeat(32).parse::<SessionId>().unwrap_err();
    let ids: Vec<_> = (0..256).map(|_| SessionId::new()).collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "session IDs must sort by creation"
    );
}

#[test]
fn paths_outside_sessions_do_not_parse() {
    assert_eq!(
        log::parse_path(Path::new("/home/x/sessions/notanid/session.jsonl")),
        None
    );
    assert_eq!(
        log::parse_path(Path::new(&format!(
            "/home/x/other/{}/session.jsonl",
            SessionId::from_u128(1)
        ))),
        None
    );
    assert_eq!(
        log::work_path(Path::new("/home/x/sessions/a/session.jsonl")),
        Path::new("/home/x/sessions/a/work.json")
    );
}

#[test]
fn summary_keeps_the_recent_tail_verbatim() {
    let tail = vec![
        Message::user("recent question"),
        Message::system("kept assistant".to_string()),
    ];
    let state = fold(vec![
        header(1),
        user("current", true),
        assistant("older answer", &[]),
        Line::Summary {
            text: "the older turns".to_string(),
            tail: tail.clone(),
        },
    ]);
    assert_eq!(state.history.len(), 4);
    assert_eq!(state.history[0].content, "current");
    assert_eq!(state.history[1].role, Role::System);
    assert_eq!(state.history[1].content, "the older turns");
    assert_eq!(state.history[2].content, "recent question");
    assert_eq!(state.history[3].content, "kept assistant");
    assert_eq!(state.opener, Some(0));
}

#[test]
fn open_heals_a_torn_last_line_so_resume_never_bricks() {
    use std::io::Write;

    let dir = tempfile::tempdir().expect("tempdir");
    let path = log::session_path(dir.path(), SessionId::from_u128(11));
    {
        let mut log = Log::create(&path).expect("create");
        log.append(&header(11)).expect("header");
        log.append(&user("first", true)).expect("first");
    }
    let torn = serde_json::to_string(&user("second", false)).expect("encode");
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        file.write_all(torn.as_bytes()).expect("write torn");
    }
    {
        let mut log = Log::open(&path).expect("open heals");
        log.append(&user("third", false))
            .expect("append after heal");
    }
    let lines = log::read(&path).expect("read");
    assert_eq!(
        lines.len(),
        4,
        "torn-but-complete line kept, no weld: {lines:?}"
    );

    let path = log::session_path(dir.path(), SessionId::from_u128(12));
    {
        let mut log = Log::create(&path).expect("create");
        log.append(&header(12)).expect("header");
        log.append(&user("only", true)).expect("only");
    }
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        file.write_all(br#"{"kind":"user","text":"tor"#)
            .expect("write partial");
    }
    {
        let mut log = Log::open(&path).expect("open heals");
        log.append(&user("next", false)).expect("append after heal");
    }
    let lines = log::read(&path).expect("read");
    assert_eq!(lines.len(), 3, "partial tail dropped: {lines:?}");
}

#[test]
fn a_torn_multibyte_last_line_does_not_brick_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("torn.jsonl");
    let mut bytes = serde_json::to_vec(&header(1)).expect("encode");
    bytes.push(b'\n');
    bytes.extend_from_slice(b"{\"kind\":\"user\",\"text\":\"caf\xc3");
    std::fs::write(&path, &bytes).expect("write");
    let lines = log::read(&path).expect("read must recover, not brick");
    assert_eq!(lines.len(), 1);
}
