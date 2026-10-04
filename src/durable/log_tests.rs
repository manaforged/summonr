use super::*;
use crate::provider::{Checked, ToolFunction, ToolKind};
use crate::tools::Event;

fn answer(tool_calls: Vec<ToolCall>, continuation: Option<Continuation>) -> Line {
    Line::Assistant {
        text: "Here".to_string(),
        reasoning: String::new(),
        tool_calls,
        usage: Usage {
            input: 10,
            output: 2,
            ..Usage::default()
        },
        model: "gpt-5.1".to_string(),
        prefix_hash: String::new(),
        cost: None,
        continuation,
    }
}

#[test]
fn a_tool_round_reports_usage_before_its_calls() {
    let call = ToolCall {
        id: "c1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    };
    let events = answer(vec![call], None).events();
    let usage = events
        .iter()
        .position(|event| matches!(event, Event::Usage { .. }))
        .expect("a usage event");
    let called = events
        .iter()
        .position(|event| matches!(event, Event::ToolCall { .. }))
        .expect("a tool call event");
    assert!(usage < called, "got {events:?}");
}

#[test]
fn a_resumed_session_keeps_the_reply_phase_and_commentary() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    let said = Continuation::Responses {
        model: "gpt-5.1".to_string(),
        signatures: Vec::new(),
        phase: Some("final_answer".to_string()),
        commentary: vec!["Got it".to_string()],
    };
    std::fs::write(&path, "").unwrap();
    let mut log = Log::open(&path).unwrap();
    log.append(&answer(Vec::new(), Some(said.clone()))).unwrap();
    drop(log);
    let mut state = State::default();
    for line in read(&path).unwrap() {
        state.apply(line);
    }
    let last = state.history.last().expect("the reply is in history");
    assert_eq!(last.continuation.as_ref(), Some(&said));
}

#[test]
fn clear_forgets_why_the_last_turn_stopped() {
    let mut state = State::default();
    state.apply(serde_json::from_str(r#"{"kind":"close","reason":"cancelled by user"}"#).unwrap());
    state.apply(Line::Clear);
    assert_eq!(state.last_close_reason, None);
}

#[test]
fn a_replayed_tool_call_is_checked_once_on_apply() {
    let call = |arguments: &str| ToolCall {
        id: "c1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    };
    let mut state = State::default();
    state.apply(answer(vec![call("{\"path\":\"a\"}"), call("{nope")], None));
    let calls = &state.history.last().unwrap().tool_calls;
    assert_eq!(calls[0].function.checked, Checked::Valid);
    assert_eq!(calls[1].function.checked, Checked::Invalid);
    assert_eq!(
        calls[1].function.arguments, "{nope",
        "the raw text stays for repair"
    );
}

#[test]
fn a_tool_line_replays_its_display_patch_and_older_lines_still_load() {
    let line = Line::Tool {
        call: "c1".to_string(),
        text: "edited a.py".to_string(),
        success: true,
        mutated: true,
        millis: 3,
        patch: Some("@@ -1 +1 @@\n-a\n+b".to_string()),
    };
    let stored: Line = serde_json::from_str(&serde_json::to_string(&line).unwrap()).unwrap();
    let Some(Event::ToolResult { patch, .. }) = stored.events().into_iter().next() else {
        panic!("expected a tool result");
    };
    assert_eq!(patch.as_deref(), Some("@@ -1 +1 @@\n-a\n+b"));
    let older: Line = serde_json::from_str(
        r#"{"kind":"tool","call":"c1","text":"edited a.py","success":true,"millis":3}"#,
    )
    .unwrap();
    let Some(Event::ToolResult { patch, .. }) = older.events().into_iter().next() else {
        panic!("expected a tool result");
    };
    assert_eq!(patch, None);
}

#[test]
fn a_summary_keeps_a_later_message_that_repeats_the_opener() {
    let user = |text: &str, start: bool| Line::User {
        text: text.to_string(),
        source: None,
        images: Vec::new(),
        start,
    };
    let mut state = State::default();
    state.apply(user("continue", true));
    state.apply(answer(Vec::new(), None));
    state.apply(user("continue", false));
    let reply = state.history[1].clone();
    let repeat = state.history[2].clone();
    state.apply(Line::Summary {
        text: "summary".to_string(),
        tail: vec![reply, repeat],
    });
    let texts: Vec<&str> = state.history.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(
        texts,
        ["continue", "summary", "Here", "continue"],
        "got {texts:?}"
    );
}

#[test]
fn records_skip_a_malformed_line_in_the_middle_of_the_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.jsonl");
    std::fs::write(
        &path,
        "{\"kind\":\"clear\"}\n{\"kind\":\"clear\"\n{\"kind\":\"clear\"}\n",
    )
    .unwrap();
    let lines = read(&path).unwrap();
    assert_eq!(lines, [Line::Clear, Line::Clear]);
}
