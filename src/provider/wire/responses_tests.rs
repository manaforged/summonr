use serde_json::json;

use super::*;
use crate::provider::{
    FunctionDefinition, StopReason, ToolCall, ToolDefinition, ToolFunction, ToolKind,
};

fn message(role: Role, content: &str) -> Message {
    Message {
        role,
        content: content.to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning: None,
        failed: false,
        continuation: None,
    }
}

fn ask<'a>(messages: &'a [Message], effort: Option<Effort>) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        "gpt-5.1".to_string(),
        messages,
        &[],
        None,
        true,
        effort,
    )
}

fn stream(events: &[serde_json::Value]) -> crate::error::Result<AssistantMessage> {
    let mut stream = <Stream as Accumulate>::new("gpt-5.1");
    for event in events {
        stream.on(&event.to_string())?;
    }
    Ok(stream.finish())
}

#[test]
fn an_error_event_reports_the_providers_message() {
    let error = stream(&[json!({"type": "error", "error": {
        "type": "insufficient_quota", "code": "insufficient_quota",
        "message": "You exceeded your current quota"}})])
    .expect_err("an error event is not a reply");
    assert!(
        error.to_string().contains("exceeded your current quota"),
        "got {error}"
    );
}

#[test]
fn commentary_streams_as_reasoning_and_the_reply_stands_alone() {
    let reply = stream(&[
        json!({"type": "response.output_item.added",
               "item": {"type": "message", "id": "m1", "phase": "commentary"}}),
        json!({"type": "response.output_text.delta", "item_id": "m1", "delta": "Got it"}),
        json!({"type": "response.output_item.added",
               "item": {"type": "message", "id": "m2", "phase": "final_answer"}}),
        json!({"type": "response.output_text.delta", "item_id": "m2", "delta": "Here"}),
        json!({"type": "response.completed", "response": {"status": "completed"}}),
    ])
    .unwrap();
    assert_eq!(reply.content.as_deref(), Some("Here"));
    assert_eq!(reply.reasoning_content.as_deref(), Some("Got it"));
}

#[test]
fn the_completed_event_does_not_repeat_streamed_text() {
    let reply = stream(&[
        json!({"type": "response.output_text.delta", "item_id": "m1", "delta": "Hi"}),
        json!({"type": "response.completed", "response": {"status": "completed", "output": [
            {"type": "message", "content": [{"type": "output_text", "text": "Hi"}]}]}}),
    ])
    .unwrap();
    assert_eq!(reply.content.as_deref(), Some("Hi"));
}

#[test]
fn a_refusal_fails_as_blocked() {
    let error = stream(&[json!({"type": "response.refusal.delta", "delta": "I can't help"})])
        .expect_err("a refusal is not a reply");
    assert!(error.to_string().contains("I can't help"), "got {error}");
}

#[test]
fn an_incomplete_reply_with_a_call_stays_a_length_stop() {
    let reply = stream(&[
        json!({"type": "response.output_item.added", "item": {
            "type": "function_call", "id": "fc1", "call_id": "c1", "name": "read"}}),
        json!({"type": "response.function_call_arguments.delta", "item_id": "fc1",
               "delta": "{\"path\""}),
        json!({"type": "response.incomplete", "response": {"status": "incomplete",
               "incomplete_details": {"reason": "max_output_tokens"}}}),
    ])
    .unwrap();
    assert_eq!(reply.stop_reason, StopReason::Length);
}

#[test]
fn a_reasoning_model_always_asks_for_a_summary_and_encrypted_content() {
    let messages = vec![message(Role::User, "hi")];
    let with = request(&ask(&messages, Some(Effort::High)), true).unwrap();
    assert_eq!(with["reasoning"]["summary"], "auto");
    assert_eq!(with["include"][0], "reasoning.encrypted_content");
    let default = request(&ask(&messages, None), true).unwrap();
    assert_eq!(default["reasoning"]["summary"], "auto", "{default}");
    assert!(default["reasoning"].get("effort").is_none(), "{default}");
    assert_eq!(default["include"][0], "reasoning.encrypted_content");
    let mut chat = ask(&messages, None);
    chat.model = "gpt-4o".to_string();
    chat.traits = crate::provider::traits("gpt-4o");
    let plain = request(&chat, true).unwrap();
    assert!(plain.get("reasoning").is_none(), "{plain}");
    assert!(plain.get("include").is_none(), "{plain}");
}

#[test]
fn a_function_tool_opts_out_of_strict_mode() {
    let messages = vec![message(Role::User, "hi")];
    let tools = vec![ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: "read".to_string(),
            description: "read".to_string(),
            parameters: json!({"type": "object"}),
        },
    }];
    let mut call = ask(&messages, None);
    call.tools = &tools;
    let body = request(&call, true).unwrap();
    assert_eq!(body["tools"][0]["strict"], json!(false), "{body}");
    assert_eq!(body["tools"][0]["name"], "read");
}

#[test]
fn a_system_note_after_the_first_turn_is_a_developer_message() {
    let messages = vec![
        message(Role::System, "be terse"),
        message(Role::User, "hi"),
        message(Role::System, "plan note"),
    ];
    let body = request(&ask(&messages, None), true).unwrap();
    assert_eq!(body["input"][0]["content"][0]["text"], "be terse");
    assert_eq!(body["input"][2]["role"], "developer");
    assert!(
        body["input"][2]["content"][0]
            .get("prompt_cache_breakpoint")
            .is_none(),
        "{body}"
    );
}

#[test]
fn fixed_prompt_is_a_plain_developer_block() {
    let messages = vec![
        message(Role::System, "be terse"),
        message(Role::System, "use tools"),
        message(Role::User, "hi"),
    ];
    let call = ask(&messages, None).with_model("gpt-6.1-sol".to_string());
    let body = request(&call, true).unwrap();
    assert!(body.get("instructions").is_none(), "{body}");
    assert_eq!(body["input"][0]["role"], "developer");
    assert_eq!(
        body["input"][0]["content"][0]["text"],
        "be terse\nuse tools"
    );
    assert!(
        body["input"][0]["content"][0]
            .get("prompt_cache_breakpoint")
            .is_none(),
        "{body}"
    );
    assert_eq!(body["input"][1]["role"], "user");
}

#[test]
fn a_replayed_reply_carries_its_commentary_and_phase() {
    let mut assistant = message(Role::Assistant, "Here");
    assistant.continuation = Some(Continuation::Responses {
        model: "gpt-5.1".to_string(),
        signatures: Vec::new(),
        phase: Some("final_answer".to_string()),
        commentary: vec!["Got it".to_string()],
    });
    let messages = vec![
        message(Role::User, "hi"),
        assistant,
        message(Role::User, "more"),
    ];
    let body = request(&ask(&messages, None), true).unwrap();
    assert_eq!(body["input"][1]["phase"], "commentary");
    assert_eq!(body["input"][1]["content"][0]["text"], "Got it");
    assert_eq!(body["input"][2]["phase"], "final_answer");
}

#[test]
fn a_call_with_an_empty_id_is_refused_before_sending() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![ToolCall {
        id: String::new(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "read".to_string(),
            arguments: "{}".to_string(),
            checked: Default::default(),
        },
        continuation: None,
    }];
    let messages = vec![message(Role::User, "hi"), assistant];
    let error = request(&ask(&messages, None), true).expect_err("an empty call_id is a 400");
    assert!(error.to_string().contains("call_id"), "got {error}");
}

#[test]
fn a_tool_result_carries_its_images() {
    let mut call = message(Role::Assistant, "");
    call.tool_calls = vec![ToolCall {
        id: "call_1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    }];
    let mut result = message(Role::Tool, "shot.png is attached");
    result.tool_call_id = Some("call_1".to_string());
    result.images = vec![crate::provider::Image::encode("image/png", b"png bytes")];
    let messages = [message(Role::User, "look"), call, result];
    let body = Wire::request(&ask(&messages, None), true).unwrap();
    let output = body["input"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .unwrap()["output"]
        .clone();
    assert_eq!(
        output,
        json!([
            {"type": "input_text", "text": "shot.png is attached"},
            {"type": "input_image", "image_url": "data:image/png;base64,cG5nIGJ5dGVz"}
        ])
    );
}
