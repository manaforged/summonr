use super::*;

use serde_json::json;

use crate::provider::{Effort, Message, Role};

fn ask(effort: Option<Effort>) -> CompletionRequest {
    CompletionRequest {
        session: "s".to_string(),
        model: "glm-5.3".to_string(),
        messages: vec![Message {
            role: Role::User,
            content: "hi".to_string(),
            images: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
        }],
        tools: Vec::new(),
        max_output_tokens: Some(256),
        emit_deltas: false,
        effort,
    }
}

#[test]
fn a_streamed_request_asks_for_the_usage_it_would_otherwise_lose() {
    let body = Wire::request(&ask(None), true);
    assert_eq!(body["stream"], true);
    assert_eq!(
        body["stream_options"]["include_usage"], true,
        "without this the final usage never arrives on a streamed turn"
    );
    assert!(Wire::request(&ask(None), false)["stream_options"].is_null());
}

#[test]
fn effort_reaches_this_wire_under_its_own_name() {
    let body = Wire::request(&ask(Some(Effort::High)), false);
    assert_eq!(body["reasoning_effort"], "high");
    assert!(
        body.get("output_config").is_none(),
        "that spelling belongs to another wire"
    );
}

#[test]
fn this_wire_is_the_one_that_ends_on_a_marker() {
    assert_eq!(Wire::sentinel(), Some("[DONE]"));
    assert_eq!(Wire::scheme(), Scheme::Bearer);
    assert_eq!(Wire::path("glm-5.3", true), "/chat/completions");
}

#[test]
fn a_whole_reply_carries_its_finish_reason_and_usage() {
    let message = Wire::parse(
        "glm-5.3",
        &json!({
            "choices": [{
                "message": {"content": "hello"},
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 5, "completion_tokens": 2}
        }),
    );
    assert_eq!(message.content.as_deref(), Some("hello"));
    assert_eq!(message.stop_reason, StopReason::ToolCalls);
    assert_eq!(message.usage.prompt_tokens, 5);
}

#[test]
fn fragmented_arguments_join_across_frames() {
    let mut stream = <Stream as Accumulate>::new("glm-5.3");
    for frame in [
        json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "id": "c1", "function": {"name": "read", "arguments": "{\"pa"}}
        ]}}]}),
        json!({"choices": [{"delta": {"tool_calls": [
            {"index": 0, "function": {"arguments": "th\":\"a.rs\"}"}}
        ]}}]}),
        json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}],
               "usage": {"prompt_tokens": 9, "completion_tokens": 4}}),
    ] {
        drop(stream.on(&frame).unwrap());
    }
    let message = stream.finish();
    assert_eq!(message.stop_reason, StopReason::ToolCalls);
    let parsed: Value =
        serde_json::from_str(&message.tool_calls[0].function.arguments).expect("joined arguments");
    assert_eq!(parsed["path"], "a.rs");
    assert_eq!(message.usage.completion_tokens, 4);
}

#[test]
fn text_and_reasoning_both_reach_the_caller() {
    let mut stream = <Stream as Accumulate>::new("glm-5.3");
    let thinking = stream
        .on(&json!({"choices": [{"delta": {"reasoning_content": "weighing"}}]}))
        .unwrap();
    let text = stream
        .on(&json!({"choices": [{"delta": {"content": "hello"}}]}))
        .unwrap();
    assert!(matches!(thinking.first(), Some(Delta::Thinking(text)) if text == "weighing"));
    assert!(matches!(text.first(), Some(Delta::Text(text)) if text == "hello"));
    let message = stream.finish();
    assert_eq!(message.reasoning_content.as_deref(), Some("weighing"));
    assert_eq!(message.content.as_deref(), Some("hello"));
}
