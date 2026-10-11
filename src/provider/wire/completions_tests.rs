use super::*;

use serde_json::json;

use crate::provider::{Effort, Message};

fn ask<'a>(effort: Option<Effort>, messages: &'a [Message]) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        "glm-5.3".to_string(),
        messages,
        &[],
        Some(256),
        false,
        effort,
    )
}

fn hi() -> [Message; 1] {
    [Message::user("hi".to_string())]
}

#[test]
fn a_streamed_request_asks_for_the_usage_it_would_otherwise_lose() {
    let body = Wire::request(&ask(None, &hi()), true).unwrap();
    assert_eq!(body["stream"], true);
    assert_eq!(
        body["stream_options"]["include_usage"], true,
        "without this the final usage never arrives on a streamed turn"
    );
    assert!(Wire::request(&ask(None, &hi()), false).unwrap()["stream_options"].is_null());
}

#[test]
fn effort_reaches_this_wire_under_its_own_name() {
    let body = Wire::request(&ask(Some(Effort::High), &hi()), false).unwrap();
    assert_eq!(body["reasoning_effort"], "high");
    assert!(
        body.get("output_config").is_none(),
        "that spelling belongs to another wire"
    );
}

#[test]
fn this_wire_is_the_one_that_ends_on_a_marker() {
    assert_eq!(Wire::sentinel(), Some("[DONE]"));
    assert_eq!(
        crate::credentials::scheme(crate::endpoint::Wire::Completions),
        summonr_auth::Scheme::Bearer
    );
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
        drop(stream.on(&frame.to_string()).unwrap());
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
        .on(&json!({"choices": [{"delta": {"reasoning_content": "weighing"}}]}).to_string())
        .unwrap();
    let text = stream
        .on(&json!({"choices": [{"delta": {"content": "hello"}}]}).to_string())
        .unwrap();
    assert!(matches!(thinking.first(), Some(Delta::Thinking(text)) if text == "weighing"));
    assert!(matches!(text.first(), Some(Delta::Text(text)) if text == "hello"));
    let message = stream.finish();
    assert_eq!(message.reasoning_content.as_deref(), Some("weighing"));
    assert_eq!(message.content.as_deref(), Some("hello"));
}

#[test]
fn a_content_filter_stop_keeps_the_text() {
    let mut stream = <Stream as Accumulate>::new("glm-5.3");
    stream
        .on(&json!({"choices": [{"delta": {"content": "partial answer"}}]}).to_string())
        .unwrap();
    stream
        .on(&json!({"choices": [{"delta": {}, "finish_reason": "content_filter"}]}).to_string())
        .expect("a filtered stop is not an error on this wire");
    let message = stream.finish();
    assert_eq!(message.content.as_deref(), Some("partial answer"));
    assert_ne!(message.stop_reason, crate::provider::StopReason::Stop);
}

fn streamed(frames: &[serde_json::Value]) -> crate::error::Result<AssistantMessage> {
    let mut stream = Stream::new("m");
    for frame in frames {
        stream.on(&frame.to_string())?;
    }
    assert!(stream.finished());
    Ok(stream.finish())
}

fn said(content: serde_json::Value, finish: Option<&str>) -> serde_json::Value {
    json!({"choices": [{"delta": {"content": content}, "finish_reason": finish}]})
}

fn kind(error: crate::error::Error) -> crate::error::ProviderFailureKind {
    error.provider_failure().unwrap().kind
}

#[test]
fn mistral_thinking_chunks_stream_as_reasoning_then_text() {
    let reply = streamed(&[
        said(
            json!([{"type": "thinking", "thinking": [{"type": "text", "text": "weigh it"}]}]),
            None,
        ),
        said(
            json!([
                {"type": "thinking", "thinking": [{"type": "text", "text": " twice"}]},
                {"type": "text", "text": "Hi"}
            ]),
            None,
        ),
        said(json!(" there"), Some("stop")),
    ])
    .unwrap();
    assert_eq!(reply.content.as_deref(), Some("Hi there"));
    assert_eq!(reply.reasoning_content.as_deref(), Some("weigh it twice"));
}

#[test]
fn a_finish_cut_by_the_server_is_a_server_failure() {
    for reason in ["network_error", "insufficient_system_resource"] {
        let error = streamed(&[said(json!("half an ans"), Some(reason))]).unwrap_err();
        assert_eq!(
            kind(error),
            crate::error::ProviderFailureKind::Server,
            "{reason}"
        );
    }
}

#[test]
fn a_full_context_window_finish_is_a_length_stop() {
    let reply = streamed(&[said(json!("so far"), Some("model_context_window_exceeded"))]).unwrap();
    assert_eq!(reply.stop_reason, StopReason::Length);
}

#[test]
fn a_refusal_is_an_error_not_an_empty_reply() {
    let error = streamed(&[
        json!({"choices": [{"delta": {"refusal": "I can't help with that."}, "finish_reason": null}]}),
        said(serde_json::Value::Null, Some("stop")),
    ])
    .unwrap_err();
    assert!(
        error.to_string().contains("I can't help with that."),
        "{error}"
    );
}
