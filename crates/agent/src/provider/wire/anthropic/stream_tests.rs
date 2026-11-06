use super::*;

use crate::provider::wire::anthropic::request;

use serde_json::json;

use crate::provider::wire::anthropic::tests::{ask, call, message};
use crate::provider::wire::{Accumulate, Delta};
use crate::provider::{Role, StopReason};

fn feed(stream: &mut Stream, events: &[Value]) -> Vec<String> {
    let mut seen = Vec::new();
    for event in events {
        match stream.on(&event.to_string()).unwrap().into_iter().next() {
            Some(Delta::Text(chunk)) => seen.push(format!("text:{chunk}")),
            Some(Delta::Thinking(chunk)) => seen.push(format!("think:{chunk}")),
            Some(Delta::Call(name)) => seen.push(format!("call:{name}")),
            Some(Delta::Progress) | None => {}
        }
    }
    seen
}

#[test]
fn a_streamed_reply_rebuilds_the_text_and_its_usage() {
    let mut stream = Stream::new("claude-opus-5");
    let deltas = feed(
        &mut stream,
        &[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 12}}}),
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "text_delta", "text": "Hel"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "text_delta", "text": "lo"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                   "usage": {"output_tokens": 12}}),
            json!({"type": "message_stop"}),
        ],
    );
    assert_eq!(
        deltas,
        vec!["text:Hel", "text:lo"],
        "every chunk is emitted"
    );
    let message = stream.finish();
    assert_eq!(message.content.as_deref(), Some("Hello"));
    assert_eq!(message.stop_reason, StopReason::Stop);
    assert_eq!(
        message.usage.prompt_tokens, 12,
        "usage starts at message_start"
    );
    assert_eq!(
        message.usage.completion_tokens, 12,
        "output tokens only arrive at message_delta"
    );
}

#[test]
fn tool_arguments_arrive_in_pieces_and_are_joined_before_use() {
    let mut stream = Stream::new("claude-opus-5");
    feed(
        &mut stream,
        &[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "tool_use", "id": "toolu_1", "name": "read"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "input_json_delta", "partial_json": "{\"pa"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "input_json_delta", "partial_json": "th\":\"a.rs\"}"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}}),
        ],
    );
    let message = stream.finish();
    assert_eq!(message.stop_reason, StopReason::ToolCalls);
    assert_eq!(message.tool_calls.len(), 1);
    let parsed: Value = serde_json::from_str(&message.tool_calls[0].function.arguments)
        .expect("the joined fragments must be valid JSON");
    assert_eq!(parsed["path"], "a.rs");
}

#[test]
fn a_tool_call_with_no_arguments_still_parses() {
    let mut stream = Stream::new("claude-opus-5");
    feed(
        &mut stream,
        &[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "tool_use", "id": "toolu_2", "name": "now"}}),
            json!({"type": "content_block_stop", "index": 0}),
        ],
    );
    let message = stream.finish();
    assert_eq!(
        message.tool_calls[0].function.arguments, "{}",
        "an empty argument stream must not yield unparseable arguments"
    );
}

#[test]
fn thinking_text_and_its_signature_are_both_kept() {
    let mut stream = Stream::new("claude-opus-5");
    let deltas = feed(
        &mut stream,
        &[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "thinking", "thinking": "", "signature": "sig-"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "thinking_delta", "thinking": "weighing"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "signature_delta", "signature": "abc"}}),
            json!({"type": "content_block_stop", "index": 0}),
        ],
    );
    assert_eq!(deltas, vec!["think:weighing"]);
    assert_eq!(
        stream.signature(),
        "sig-abc",
        "a signature arrives in pieces and is replayed with the thinking it signs"
    );
    assert_eq!(
        stream.finish().reasoning_content.as_deref(),
        Some("weighing")
    );
}

#[test]
fn two_calls_in_one_reply_stay_separate() {
    let mut stream = Stream::new("claude-opus-5");
    feed(
        &mut stream,
        &[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "tool_use", "id": "a", "name": "one"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"partial_json": "{\"x\":1}"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1,
                   "content_block": {"type": "tool_use", "id": "b", "name": "two"}}),
            json!({"type": "content_block_delta", "index": 1,
                   "delta": {"partial_json": "{\"y\":2}"}}),
            json!({"type": "content_block_stop", "index": 1}),
        ],
    );
    let message = stream.finish();
    assert_eq!(message.tool_calls.len(), 2);
    assert_eq!(message.tool_calls[0].id, "a");
    assert_eq!(message.tool_calls[1].function.arguments, r#"{"y":2}"#);
}

#[test]
fn a_ping_or_an_unknown_event_changes_nothing() {
    let mut stream = Stream::new("claude-opus-5");
    feed(
        &mut stream,
        &[
            json!({"type": "ping"}),
            json!({"type": "something_new_next_year"}),
            json!({"not_even_typed": true}),
        ],
    );
    let message = stream.finish();
    assert!(message.content.is_none());
    assert!(message.tool_calls.is_empty());
}

#[test]
fn a_stream_cut_before_it_ends_keeps_what_arrived() {
    let mut stream = Stream::new("claude-opus-5");
    feed(
        &mut stream,
        &[
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"text": "half a sen"}}),
        ],
    );
    assert_eq!(
        stream.finish().content.as_deref(),
        Some("half a sen"),
        "a cut stream returns partial output rather than nothing"
    );
}

#[test]
fn a_kept_thinking_block_is_replayed_whole_on_the_next_turn() {
    let mut assistant = message(Role::Assistant, "Reading it.");
    assistant.tool_calls = vec![ToolCall {
        id: "toolu_1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "read".to_string(),
            arguments: r#"{"path":"a.rs"}"#.to_string(),
            checked: Default::default(),
        },
        continuation: Some(crate::provider::Continuation::Anthropic {
            model: "claude-opus-5".to_string(),
            blocks: vec![
                json!({"type": "thinking", "thinking": "weighing", "signature": "sig-1"}),
                json!({"type": "text", "text": "Reading it."}),
                json!({"type": "tool_use", "id": "toolu_1", "name": "read",
                       "input": {"path": "a.rs"}}),
            ],
        }),
    }];
    let body = request(&ask(&[message(Role::User, "go"), assistant]), false).unwrap();
    let parts = body["messages"][1]["content"].as_array().unwrap();
    assert_eq!(
        parts[0]["type"], "thinking",
        "a reasoning turn must go back with its thinking, or the model degrades"
    );
    assert_eq!(
        parts[0]["signature"], "sig-1",
        "the signature is what makes the thinking replayable"
    );
    assert_eq!(parts[2]["type"], "tool_use");
}

#[test]
fn a_turn_with_nothing_kept_is_still_rebuilt_from_what_we_have() {
    let mut assistant = message(Role::Assistant, "hello");
    assistant.tool_calls = vec![call("toolu_9", "read", "{}")];
    let body = request(&ask(&[message(Role::User, "go"), assistant]), false).unwrap();
    let parts = body["messages"][1]["content"].as_array().unwrap();
    assert_eq!(parts[0]["text"], "hello");
    assert_eq!(parts[1]["id"], "toolu_9");
}

#[test]
fn the_final_message_delta_sets_the_prompt_count() {
    let mut stream = Stream::new("claude-sonnet-5");
    feed(
        &mut stream,
        &[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 43}}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                   "usage": {"input_tokens": 61, "output_tokens": 2}}),
            json!({"type": "message_stop"}),
        ],
    );
    assert_eq!(stream.finish().usage.prompt_tokens, 61);
}

#[test]
fn cache_reads_count_toward_the_prompt() {
    let mut stream = Stream::new("claude-sonnet-5");
    feed(
        &mut stream,
        &[
            json!({"type": "message_start", "message": {"usage": {
                "input_tokens": 10, "cache_read_input_tokens": 90}}}),
            json!({"type": "message_stop"}),
        ],
    );
    let usage = stream.finish().usage;
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.cached_tokens, 90);
}

#[test]
fn an_error_event_fails_the_turn_with_its_message() {
    let mut stream = Stream::new("claude-sonnet-5");
    let error = stream
        .on(
            &json!({"type": "error", "error": {"type": "overloaded_error",
                     "message": "Overloaded"}})
            .to_string(),
        )
        .err()
        .expect("an error event must not read as a reply");
    assert!(error.to_string().contains("Overloaded"), "got {error}");
}

#[test]
fn a_refusal_fails_with_the_providers_explanation() {
    let mut stream = Stream::new("claude-sonnet-5");
    let error = stream
        .on(
            &json!({"type": "message_delta", "delta": {"stop_reason": "refusal",
                     "stop_details": {"explanation": "blocked by policy"}}})
            .to_string(),
        )
        .err()
        .expect("a refusal is not a clean stop");
    assert!(
        error.to_string().contains("blocked by policy"),
        "got {error}"
    );
}

#[test]
fn a_reply_cut_inside_a_tool_call_keeps_its_text_as_a_length_stop() {
    let mut stream = Stream::new("claude-sonnet-5");
    feed(
        &mut stream,
        &[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "text_delta", "text": "Reading"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1,
                   "content_block": {"type": "tool_use", "id": "t1", "name": "read"}}),
            json!({"type": "content_block_delta", "index": 1,
                   "delta": {"type": "input_json_delta", "partial_json": "{\"path\": \"a"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"},
                   "usage": {"output_tokens": 9}}),
            json!({"type": "message_stop"}),
        ],
    );
    let message = stream.finish();
    assert_eq!(message.stop_reason, StopReason::Length);
    assert_eq!(message.content.as_deref(), Some("Reading"));
    assert!(message.tool_calls.is_empty());
}

#[test]
fn a_stream_without_message_stop_is_not_finished() {
    let mut stream = Stream::new("claude-sonnet-5");
    feed(
        &mut stream,
        &[json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}})],
    );
    assert!(!stream.finished());
}
