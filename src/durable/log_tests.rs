use super::*;
use crate::provider::{ToolFunction, ToolKind};
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
