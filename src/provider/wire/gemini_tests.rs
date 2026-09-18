use super::*;

use crate::provider::{Continuation, FunctionDefinition, Image, ToolDefinition};

fn message(role: Role, content: &str) -> Message {
    Message {
        role,
        content: content.to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning: None,
    }
}

fn ask(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        session: "s".to_string(),
        model: "gemini-2.5-flash".to_string(),
        messages,
        tools: Vec::new(),
        max_output_tokens: Some(2048),
        emit_deltas: false,
        effort: None,
    }
}

#[test]
fn the_system_turn_becomes_a_system_instruction() {
    let body = request(&ask(vec![
        message(Role::System, "be terse"),
        message(Role::User, "hello"),
    ]));
    assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be terse");
    assert_eq!(body["contents"].as_array().unwrap().len(), 1);
    assert_eq!(body["contents"][0]["role"], "user");
    assert_eq!(body["generationConfig"]["maxOutputTokens"], 2048);
}

#[test]
fn two_system_turns_join_rather_than_the_second_winning() {
    let body = request(&ask(vec![
        message(Role::System, "first"),
        message(Role::System, "second"),
        message(Role::User, "hi"),
    ]));
    assert_eq!(
        body["systemInstruction"]["parts"][0]["text"], "first\nsecond",
        "Gemini takes one instruction, so dropping the earlier one would lose it"
    );
}

#[test]
fn the_assistant_is_called_model_on_this_wire() {
    let body = request(&ask(vec![
        message(Role::User, "hi"),
        message(Role::Assistant, "hello"),
    ]));
    assert_eq!(body["contents"][1]["role"], "model");
}

#[test]
fn two_results_for_one_model_turn_merge_into_a_single_user_turn() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![
        ToolCall {
            id: "one".to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: "read".to_string(),
                arguments: "{}".to_string(),
            },
            continuation: None,
        },
        ToolCall {
            id: "two".to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: "list".to_string(),
                arguments: "{}".to_string(),
            },
            continuation: None,
        },
    ];
    let mut first = message(Role::Tool, "a");
    first.tool_call_id = Some("one".to_string());
    let mut second = message(Role::Tool, "b");
    second.tool_call_id = Some("two".to_string());
    let body = request(&ask(vec![
        message(Role::User, "go"),
        assistant,
        first,
        second,
    ]));
    let turns = body["contents"].as_array().unwrap();
    assert_eq!(
        turns.len(),
        3,
        "user, model, then one user turn holding both results: {turns:?}"
    );
    let parts = turns[2]["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 2, "both responses ride in one turn");
    assert_eq!(parts[0]["functionResponse"]["name"], "one");
    assert_eq!(parts[1]["functionResponse"]["response"]["content"], "b");
}

#[test]
fn a_user_turn_and_a_following_result_do_not_open_two_turns() {
    let mut result = message(Role::Tool, "a");
    result.tool_call_id = Some("one".to_string());
    let body = request(&ask(vec![message(Role::User, "go"), result]));
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
    let body = request(&ask(vec![user]));
    let parts = body["contents"][0]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["inlineData"]["mimeType"], "image/png");
    assert!(parts[0]["inlineData"]["data"].is_string());
    assert_eq!(parts[1]["text"], "what is this");
}

#[test]
fn a_tool_is_declared_with_parameters_not_input_schema() {
    let mut call = ask(vec![message(Role::User, "hi")]);
    call.tools = vec![ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: "read".to_string(),
            description: "Read a file".to_string(),
            parameters: json!({"type": "object"}),
        },
    }];
    let body = request(&call);
    let declared = &body["tools"][0]["functionDeclarations"][0];
    assert_eq!(declared["name"], "read");
    assert_eq!(
        declared["parameters"]["type"], "object",
        "Gemini names the schema parameters, unlike Anthropic"
    );
}

#[test]
fn a_replayed_call_carries_its_thought_signature_back() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![ToolCall {
        id: "c1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "read".to_string(),
            arguments: r#"{"path":"a.rs"}"#.to_string(),
        },
        continuation: Some(Continuation::Gemini {
            model: "gemini-2.5-flash".to_string(),
            signatures: vec!["sig-1".to_string()],
        }),
    }];
    let body = request(&ask(vec![message(Role::User, "go"), assistant]));
    let parts = body["contents"][1]["parts"].as_array().unwrap();
    assert_eq!(parts[0]["functionCall"]["name"], "read");
    assert_eq!(parts[0]["functionCall"]["args"]["path"], "a.rs");
    assert_eq!(
        parts[1]["thoughtSignature"], "sig-1",
        "a reasoning model needs its own signature back or it degrades"
    );
}

#[test]
fn effort_sets_a_thinking_level_and_asks_for_the_thoughts() {
    let mut call = ask(vec![message(Role::User, "hi")]);
    call.effort = Some(Effort::High);
    let body = request(&call);
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "high"
    );
    assert_eq!(
        body["generationConfig"]["thinkingConfig"]["includeThoughts"],
        true
    );
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
    assert_eq!(reply.signatures, vec!["sig-1".to_string()]);
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
fn cached_tokens_are_not_counted_twice() {
    let counted = usage(&json!({
        "promptTokenCount": 1000,
        "cachedContentTokenCount": 800,
        "candidatesTokenCount": 20,
        "thoughtsTokenCount": 15
    }));
    assert_eq!(
        counted.prompt_tokens, 200,
        "promptTokenCount already includes the cached tokens on this wire"
    );
    assert_eq!(counted.cached_tokens, 800);
    assert_eq!(counted.reasoning_tokens, 15);
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

#[test]
fn a_response_names_the_function_not_the_call_id_we_invented() {
    let mut reply = <Reply as Accumulate>::new("gemini-2.5-flash");
    drop(reply.take(&json!({
        "candidates": [{"content": {"parts": [
            {"functionCall": {"name": "read", "args": {}}}
        ]}}]
    })));
    let id = reply.finish().tool_calls[0].id.clone();
    let mut result = message(Role::Tool, "contents");
    result.tool_call_id = Some(id.clone());
    let body = request(&ask(vec![message(Role::User, "go"), result]));
    let part = &body["contents"][0]["parts"][1];
    assert_eq!(
        part["functionResponse"]["name"], "read",
        "Gemini matches the response to the declared function, not to `{id}`"
    );
    assert_eq!(part["functionResponse"]["response"]["content"], "contents");
}

#[test]
fn a_real_call_id_is_used_as_given() {
    assert_eq!(called_name("read"), "read");
    assert_eq!(called_name("call-read-0"), "read");
    assert_eq!(called_name("call-my-tool-3"), "my-tool");
    assert_eq!(
        called_name("call-weird"),
        "call-weird",
        "an id that is not one of ours is passed through untouched"
    );
}
