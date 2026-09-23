use super::*;

use serde_json::json;

use crate::provider::{Message, Role, StopReason, ToolCall, ToolFunction, ToolKind};

fn user(text: &str) -> Message {
    Message {
        role: Role::User,
        content: text.to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning: None,
    }
}

fn ask<'a>(messages: &'a [Message]) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        "a-model".to_string(),
        messages,
        &[],
        Some(64),
        false,
        None,
    )
}

fn drive<W: Chat>(frames: &[Value]) -> (AssistantMessage, Vec<Delta>) {
    let mut stream = W::Stream::new("a-model");
    let mut deltas = Vec::new();
    for frame in frames {
        deltas.extend(stream.on(&frame.to_string()).unwrap());
    }
    (stream.finish(), deltas)
}

fn anthropic_turn() -> Vec<Value> {
    vec![
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 9}}}),
        json!({"type": "content_block_start", "index": 0,
               "content_block": {"type": "thinking", "thinking": "", "signature": "sig"}}),
        json!({"type": "content_block_delta", "index": 0,
               "delta": {"thinking": "weighing"}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1,
               "content_block": {"type": "tool_use", "id": "toolu_1", "name": "read"}}),
        json!({"type": "content_block_delta", "index": 1,
               "delta": {"partial_json": "{\"path\":\"a.rs\"}"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"},
               "usage": {"output_tokens": 4}}),
    ]
}

fn gemini_turn() -> Vec<Value> {
    vec![
        json!({"candidates": [{"content": {"parts": [
            {"thought": true, "text": "weighing"},
            {"thoughtSignature": "sig"}
        ]}}]}),
        json!({"candidates": [{"content": {"parts": [
            {"functionCall": {"name": "read", "args": {"path": "a.rs"}}}
        ]}, "finishReason": "STOP"}],
        "usageMetadata": {"promptTokenCount": 9, "candidatesTokenCount": 4}}),
    ]
}

#[test]
fn a_reply_that_wants_tools_says_so_on_every_wire() {
    let (anthropic, _) = drive::<anthropic::Wire>(&anthropic_turn());
    let (gemini, _) = drive::<gemini::Wire>(&gemini_turn());
    for (name, message) in [("anthropic", anthropic), ("gemini", gemini)] {
        assert!(!message.tool_calls.is_empty(), "{name} lost the call");
        assert_eq!(
            message.stop_reason,
            StopReason::ToolCalls,
            "{name} reported a plain stop while asking for a tool, which any caller \
             reading stop_reason would take as the end of the turn"
        );
    }
}

#[test]
fn reasoning_reaches_the_caller_as_it_arrives_on_every_wire() {
    for (name, deltas) in [
        ("anthropic", drive::<anthropic::Wire>(&anthropic_turn()).1),
        ("gemini", drive::<gemini::Wire>(&gemini_turn()).1),
    ] {
        assert!(
            deltas
                .iter()
                .any(|delta| matches!(delta, Delta::Thinking(text) if text.contains("weighing"))),
            "{name} kept its reasoning to itself, so a reader would see a long silence"
        );
    }
}

#[test]
fn a_turn_that_reasoned_carries_what_it_needs_to_be_replayed() {
    for (name, message) in [
        ("anthropic", drive::<anthropic::Wire>(&anthropic_turn()).0),
        ("gemini", drive::<gemini::Wire>(&gemini_turn()).0),
    ] {
        let continuation = message.tool_calls[0].continuation.as_ref();
        assert!(
            continuation.is_some(),
            "{name} dropped its reasoning continuation; the next turn degrades quietly"
        );
    }
}

#[test]
fn a_replayed_turn_survives_the_round_trip_with_its_reasoning() {
    let reply = drive::<anthropic::Wire>(&anthropic_turn()).0;
    let mut assistant = user("");
    assistant.role = Role::Assistant;
    assistant.tool_calls = reply.tool_calls.clone();
    let body = anthropic::Wire::request(&ask(&[user("go"), assistant]), false);
    let parts = body["messages"][1]["content"].as_array().unwrap();
    assert_eq!(parts[0]["type"], "thinking", "got {parts:?}");
    assert_eq!(parts[0]["signature"], "sig");
    assert_eq!(parts[1]["type"], "tool_use");
    assert_eq!(
        parts[1]["input"]["path"], "a.rs",
        "the replayed call must carry the arguments the model chose"
    );
}

#[test]
fn a_result_can_be_matched_back_to_the_call_that_asked_for_it() {
    for (name, message, wire) in [
        (
            "anthropic",
            drive::<anthropic::Wire>(&anthropic_turn()).0,
            0,
        ),
        ("gemini", drive::<gemini::Wire>(&gemini_turn()).0, 1),
    ] {
        let id = message.tool_calls[0].id.clone();
        assert!(!id.is_empty(), "{name} gave the call no id to answer");
        let mut result = user("done");
        result.role = Role::Tool;
        result.tool_call_id = Some(id);
        let body = if wire == 0 {
            anthropic::Wire::request(&ask(&[user("go"), result]), false)
        } else {
            gemini::Wire::request(&ask(&[user("go"), result]), false)
        };
        let text = body.to_string();
        assert!(
            text.contains("read") || text.contains("toolu_1"),
            "{name} sent a result the provider cannot match to a call: {text}"
        );
    }
}

#[test]
fn every_wire_counts_the_same_reply_the_same_way() {
    let (anthropic, _) = drive::<anthropic::Wire>(&anthropic_turn());
    let (gemini, _) = drive::<gemini::Wire>(&gemini_turn());
    assert_eq!(
        (
            anthropic.usage.prompt_tokens,
            anthropic.usage.completion_tokens
        ),
        (9, 4)
    );
    assert_eq!(
        (gemini.usage.prompt_tokens, gemini.usage.completion_tokens),
        (9, 4),
        "the same token counts must land in the same fields, or cost reporting \
         silently depends on which wire served the turn"
    );
}

#[test]
fn a_stream_that_stops_early_still_yields_what_arrived() {
    for (name, message) in [
        (
            "anthropic",
            drive::<anthropic::Wire>(&anthropic_turn()[..3]).0,
        ),
        ("gemini", drive::<gemini::Wire>(&gemini_turn()[..1]).0),
    ] {
        assert!(
            message.reasoning_content.is_some(),
            "{name} threw away a cut turn instead of returning the part that arrived"
        );
    }
}

#[test]
fn an_unknown_frame_never_breaks_a_wire() {
    let noise = vec![
        json!({}),
        json!({"type": "a_new_event_type"}),
        json!({"candidates": []}),
        json!(null),
    ];
    assert!(drive::<anthropic::Wire>(&noise).0.content.is_none());
    assert!(drive::<gemini::Wire>(&noise).0.content.is_none());
}

#[test]
fn a_tool_call_is_always_answerable_and_always_parseable() {
    let bare = vec![json!({"candidates": [{"content": {"parts": [
        {"functionCall": {"name": "now"}},
        {"functionCall": {"name": "now"}}
    ]}}]})];
    let message = drive::<gemini::Wire>(&bare).0;
    let mut seen = std::collections::BTreeSet::new();
    for call in &message.tool_calls {
        assert!(seen.insert(call.id.clone()), "two calls shared one id");
        serde_json::from_str::<Value>(&call.function.arguments)
            .expect("arguments must parse even when the wire sent none");
    }
}

#[test]
fn a_request_never_loses_a_turn() {
    let turns = vec![
        user("first"),
        {
            let mut assistant = user("answer");
            assistant.role = Role::Assistant;
            assistant.tool_calls = vec![ToolCall {
                id: "c1".to_string(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    name: "read".to_string(),
                    arguments: "{}".to_string(),
                },
                continuation: None,
            }];
            assistant
        },
        {
            let mut result = user("contents");
            result.role = Role::Tool;
            result.tool_call_id = Some("c1".to_string());
            result
        },
        user("second"),
    ];
    let anthropic = anthropic::Wire::request(&ask(&turns), false).to_string();
    let gemini = gemini::Wire::request(&ask(&turns), false).to_string();
    for (name, body) in [("anthropic", anthropic), ("gemini", gemini)] {
        for needle in ["first", "answer", "contents", "second"] {
            assert!(
                body.contains(needle),
                "{name} dropped `{needle}` from the conversation it sent"
            );
        }
    }
}

fn continued(message: &AssistantMessage) -> Option<&crate::provider::Continuation> {
    message.tool_calls.first()?.continuation.as_ref()
}

#[test]
fn a_continuation_names_the_model_that_produced_it() {
    let mut anthropic = anthropic::Stream::new("claude-opus-5");
    for frame in anthropic_turn() {
        drop(anthropic.on(&frame.to_string()).unwrap());
    }
    match continued(&anthropic.finish()) {
        Some(crate::provider::Continuation::Anthropic { model, blocks }) => {
            assert_eq!(
                model, "claude-opus-5",
                "a continuation with no model is discarded by the replay guard, \
                 so the reasoning is lost without any error"
            );
            assert!(!blocks.is_empty());
        }
        other => panic!("anthropic built {other:?}"),
    }

    let mut gemini = <gemini::Reply as Accumulate>::new("gemini-2.5-flash");
    for frame in gemini_turn() {
        drop(gemini.on(&frame.to_string()).unwrap());
    }
    match continued(&gemini.finish()) {
        Some(crate::provider::Continuation::Gemini { model, signatures }) => {
            assert_eq!(model, "gemini-2.5-flash");
            assert!(!signatures.is_empty());
        }
        other => panic!("gemini built {other:?}"),
    }
}

#[test]
fn a_whole_reply_and_a_streamed_one_agree_on_every_wire() {
    let streamed = drive::<anthropic::Wire>(&anthropic_turn()).0;
    let whole = anthropic::Wire::parse(
        "claude-opus-5",
        &json!({
            "content": [
                {"type": "thinking", "thinking": "weighing", "signature": "sig"},
                {"type": "tool_use", "id": "toolu_1", "name": "read",
                 "input": {"path": "a.rs"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 9, "output_tokens": 4}
        }),
    );
    assert_eq!(whole.reasoning_content, streamed.reasoning_content);
    assert_eq!(whole.stop_reason, streamed.stop_reason);
    assert_eq!(whole.usage, streamed.usage);
    assert_eq!(
        whole.tool_calls[0].function.arguments,
        streamed.tool_calls[0].function.arguments
    );
    assert!(
        continued(&whole).is_some(),
        "a reply that did not stream must still be replayable, or reasoning \
         survives only on the streaming path"
    );
}
