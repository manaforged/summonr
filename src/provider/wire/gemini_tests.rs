use super::*;

use crate::provider::{Effort, Image};

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

fn ask<'a>(messages: &'a [Message]) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        "gemini-2.5-flash".to_string(),
        messages,
        &[],
        Some(2048),
        false,
        None,
    )
}

#[test]
fn the_system_turn_becomes_a_system_instruction() {
    let body = request(&ask(&[
        message(Role::System, "be terse"),
        message(Role::User, "hello"),
    ]))
    .unwrap();
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be terse");
    assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 2048);
}

#[test]
fn two_system_turns_join_rather_than_the_second_winning() {
    let body = request(&ask(&[
        message(Role::System, "first"),
        message(Role::System, "second"),
        message(Role::User, "hi"),
    ]))
    .unwrap();
    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"], "first\nsecond",
        "Gemini takes one instruction, so dropping the earlier one would lose it"
    );
}

#[test]
fn the_assistant_is_called_model_on_this_wire() {
    let body = request(&ask(&[
        message(Role::User, "hi"),
        message(Role::Assistant, "hello"),
    ]))
    .unwrap();
    assert_eq!(body["contents"][1]["role"], "model");
}

#[test]
fn a_user_turn_and_a_following_result_do_not_open_two_turns() {
    let mut result = message(Role::Tool, "a");
    result.tool_call_id = Some("one".to_string());
    let body = request(&ask(&[message(Role::User, "go"), result])).unwrap();
    assert_eq!(
        body["contents"].as_array().unwrap().len(),
        1,
        "Gemini rejects two user turns in a row, so they must join"
    );
}

#[test]
fn an_image_is_inline_data_with_its_mime_type() {
    let mut user = message(Role::User, "what is this");
    user.images = vec![Image::encode("image/png", &[1, 2, 3])];
    let body = request(&ask(&[user])).unwrap();
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
    assert!(parts[0]["inlineData"]["data"].is_string());
    assert_eq!(parts[1]["text"], "what is this");
}

#[test]
fn a_reply_parses_into_text_thinking_calls_and_signatures() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    let chunk = reply.take(&json!({
        "candidates": [{
            "content": {"parts": [
                {"thought": true, "text": "weighing"},
                {"thoughtSignature": "sig-1"},
                {"text": "Reading it."},
                {"functionCall": {"name": "read", "args": {"path": "a.rs"}}}
            ]},
            "finishReason": "STOP"
        }],
        "usageMetadata": {"promptTokenCount": 30, "candidatesTokenCount": 5}
    }));
    assert_eq!(chunk.as_deref(), Some("Reading it."));
    assert_eq!(reply.said, "sig-1");
    let message = reply.finish();
    assert_eq!(message.content.as_deref(), Some("Reading it."));
    assert_eq!(message.reasoning_content.as_deref(), Some("weighing"));
    assert_eq!(message.tool_calls[0].function.name, "read");
    assert_eq!(
        message.stop_reason,
        StopReason::ToolCalls,
        "Gemini says STOP even when it asks for a tool, so the wire must correct it"
    );
}

#[test]
fn a_call_with_no_id_gets_one_that_does_not_collide() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    drop(reply.take(&json!({
        "candidates": [{"content": {"parts": [
            {"functionCall": {"name": "read", "args": {}}},
            {"functionCall": {"name": "read", "args": {}}}
        ]}}]
    })));
    let message = reply.finish();
    assert_eq!(message.tool_calls.len(), 2);
    assert_ne!(
        message.tool_calls[0].id, message.tool_calls[1].id,
        "Gemini omits call ids, so two calls to one tool must not share one"
    );
}

#[test]
fn a_call_with_null_arguments_still_parses() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    drop(reply.take(&json!({
        "candidates": [{"content": {"parts": [
            {"functionCall": {"name": "now"}}
        ]}}]
    })));
    assert_eq!(reply.finish().tool_calls[0].function.arguments, "{}");
}

#[test]
fn a_stream_of_frames_joins_into_one_reply() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    let first = reply.take(&json!({
        "candidates": [{"content": {"parts": [{"text": "Hel"}]}}]
    }));
    let second = reply.take(&json!({
        "candidates": [{"content": {"parts": [{"text": "lo"}]}}]
    }));
    let tail = reply.take(&json!({
        "candidates": [{"finishReason": "MAX_TOKENS"}],
        "usageMetadata": {"promptTokenCount": 4, "candidatesTokenCount": 2}
    }));
    assert_eq!(first.as_deref(), Some("Hel"));
    assert_eq!(second.as_deref(), Some("lo"));
    assert!(tail.is_none(), "the tail frame carries no text");
    let message = reply.finish();
    assert_eq!(message.content.as_deref(), Some("Hello"));
    assert_eq!(message.stop_reason, StopReason::Length);
    assert_eq!(message.usage.completion_tokens, 2);
}

#[test]
fn an_unknown_finish_reason_is_not_a_clean_stop() {
    assert_eq!(stop_reason("SAFETY"), StopReason::Other);
    assert_eq!(stop_reason("RECITATION"), StopReason::Other);
    assert_eq!(stop_reason("STOP"), StopReason::Stop);
}

fn with_effort<'a>(model: &str, messages: &'a [Message], effort: Effort) -> CompletionRequest<'a> {
    let mut call = ask(messages).with_model(model.to_string());
    call.effort = Some(effort);
    call
}

fn call_for(id: &str, name: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::ToolFunction {
            name: name.to_string(),
            arguments: "{}".to_string(),
            checked: Default::default(),
        },
        continuation: None,
    }
}

#[test]
fn a_replayed_call_carries_its_signature_on_the_call_part() {
    let mut assistant = message(Role::Assistant, "");
    let mut called = call_for("call-weather-0", "weather");
    called.continuation = Some(Continuation::Gemini {
        model: "gemini-2.5-flash".to_string(),
        signatures: vec!["sig-1".to_string()],
    });
    assistant.tool_calls = vec![called];
    let messages = vec![message(Role::User, "weather?"), assistant];
    let body = request(&ask(&messages)).unwrap();
    let part = &body["contents"][1]["parts"][0];
    assert_eq!(part["functionCall"]["name"], "weather");
    assert_eq!(part["thoughtSignature"], "sig-1");
}

#[test]
fn a_tool_result_names_the_function_and_keeps_a_real_call_id() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![call_for("fc_42", "weather")];
    let mut result = message(Role::Tool, "sunny");
    result.tool_call_id = Some("fc_42".to_string());
    let messages = vec![message(Role::User, "weather?"), assistant, result];
    let body = request(&ask(&messages)).unwrap();
    let response = &body["contents"][2]["parts"][0]["functionResponse"];
    assert_eq!(response["name"], "weather");
    assert_eq!(response["id"], "fc_42");
}

#[test]
fn a_tool_schema_goes_as_json_schema() {
    let messages = vec![message(Role::User, "hi")];
    let tools = vec![crate::provider::ToolDefinition {
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::FunctionDefinition {
            name: "read".to_string(),
            description: "read a file".to_string(),
            parameters: serde_json::json!({"type": "object", "additionalProperties": false}),
        },
    }];
    let mut call = ask(&messages);
    call.tools = &tools;
    let body = request(&call).unwrap();
    let declared = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(
        declared["parametersJsonSchema"]["additionalProperties"],
        false
    );
    assert!(declared.get("parameters").is_none());
}

#[test]
fn effort_none_turns_thinking_off_on_flash() {
    let messages = vec![message(Role::User, "hi")];
    let body = request(&with_effort("gemini-2.5-flash", &messages, Effort::None)).unwrap();
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingBudget"],
        0
    );
}

#[test]
fn a_thinking_level_goes_only_to_gemini_3() {
    let messages = vec![message(Role::User, "hi")];
    let three = request(&with_effort("gemini-3-flash", &messages, Effort::High)).unwrap();
    assert_eq!(
        three["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "high"
    );
    let older = request(&with_effort("gemini-2.0-flash", &messages, Effort::High)).unwrap();
    assert!(older["generationConfig"].get("thinkingConfig").is_none());
}

#[test]
fn a_blocked_prompt_fails_as_blocked() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    let error = reply
        .on(r#"{"promptFeedback":{"blockReason":"PROHIBITED_CONTENT"}}"#)
        .err()
        .expect("a blocked prompt is not an empty reply");
    assert!(
        error.to_string().contains("PROHIBITED_CONTENT"),
        "got {error}"
    );
}

#[test]
fn a_malformed_call_finish_asks_for_a_repair() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    let error = reply
        .on(r#"{"candidates":[{"finishReason":"MALFORMED_FUNCTION_CALL"}]}"#)
        .err()
        .expect("a malformed call must fail");
    let crate::error::Error::Provider(failure) = error else {
        panic!("not a provider failure");
    };
    assert_eq!(
        failure.kind,
        crate::error::ProviderFailureKind::MalformedToolCall
    );
}

#[test]
fn cached_tokens_stay_in_the_prompt_and_thoughts_count_as_output() {
    let used = usage(&serde_json::json!({
        "promptTokenCount": 100_000,
        "cachedContentTokenCount": 90_000,
        "candidatesTokenCount": 23,
        "thoughtsTokenCount": 185,
    }));
    assert_eq!(used.prompt_tokens, 100_000);
    assert_eq!(used.cached_tokens, 90_000);
    assert_eq!(used.completion_tokens, 208);
}
