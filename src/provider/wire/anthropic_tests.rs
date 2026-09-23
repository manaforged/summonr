use super::*;

use crate::provider::{FunctionDefinition, Image, ToolDefinition};

pub(super) fn message(role: Role, content: &str) -> Message {
    Message {
        role,
        content: content.to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning: None,
    }
}

pub(super) fn call(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    }
}

pub(super) fn ask<'a>(messages: &'a [Message]) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        "claude-opus-5".to_string(),
        messages,
        &[],
        Some(1024),
        false,
        None,
    )
}

#[test]
fn the_system_turn_leaves_the_message_list() {
    let body = request(
        &ask(&[
            message(Role::System, "be terse"),
            message(Role::User, "hello"),
        ]),
        false,
    );
    assert_eq!(body["system"][0]["text"], "be terse");
    assert_eq!(
        body["messages"].as_array().unwrap().len(),
        1,
        "Anthropic takes the system prompt beside the messages, not inside them"
    );
    assert_eq!(body["messages"][0]["role"], "user");
    assert_eq!(body["max_tokens"], 1024);
}

#[test]
fn a_tool_result_is_a_user_turn_carrying_the_call_id() {
    let mut result = message(Role::Tool, "72F and sunny");
    result.tool_call_id = Some("toolu_abc123".to_string());
    let body = request(&ask(&[message(Role::User, "weather?"), result]), false);
    let last = &body["messages"][1];
    assert_eq!(last["role"], "user", "a tool result is a user turn here");
    assert_eq!(last["content"][0]["type"], "tool_result");
    assert_eq!(last["content"][0]["tool_use_id"], "toolu_abc123");
}

#[test]
fn two_results_for_one_turn_ride_in_a_single_user_message() {
    let mut first = message(Role::Tool, "a");
    first.tool_call_id = Some("toolu_1".to_string());
    let mut second = message(Role::Tool, "b");
    second.tool_call_id = Some("toolu_2".to_string());
    let body = request(&ask(&[message(Role::User, "go"), first, second]), false);
    assert_eq!(
        body["messages"].as_array().unwrap().len(),
        2,
        "splitting results across turns teaches the model to stop calling in parallel"
    );
    assert_eq!(body["messages"][1]["content"].as_array().unwrap().len(), 2);
}

#[test]
fn an_assistant_turn_replays_its_tool_calls_as_blocks() {
    let mut assistant = message(Role::Assistant, "Let me check.");
    assistant.tool_calls = vec![call("toolu_abc", "get_weather", r#"{"location":"Paris"}"#)];
    let body = request(&ask(&[message(Role::User, "weather?"), assistant]), false);
    let parts = body["messages"][1]["content"].as_array().unwrap();
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "tool_use");
    assert_eq!(parts[1]["id"], "toolu_abc");
    assert_eq!(
        parts[1]["input"]["location"], "Paris",
        "arguments go back as an object, not as the string we stored"
    );
}

#[test]
fn an_image_is_a_base64_source_before_the_text() {
    let mut user = message(Role::User, "what is this");
    user.images = vec![Image::encode("image/png", &[1, 2, 3])];
    let body = request(&ask(&[user]), false);
    let parts = body["messages"][0]["content"].as_array().unwrap();
    assert_eq!(parts[0]["type"], "image");
    assert_eq!(parts[0]["source"]["media_type"], "image/png");
    assert_eq!(parts[0]["source"]["type"], "base64");
    assert_eq!(parts[1]["type"], "text");
}

#[test]
fn a_tool_is_declared_with_its_schema_under_input_schema() {
    let messages = vec![message(Role::User, "hi")];
    let tools = vec![ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: "get_weather".to_string(),
            description: "Get weather".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        },
    }];
    let mut call = ask(&messages);
    call.tools = &tools;
    let body = request(&call, false);
    assert_eq!(body["tools"][0]["name"], "get_weather");
    assert_eq!(
        body["tools"][0]["input_schema"]["type"], "object",
        "Anthropic names the schema input_schema, not parameters"
    );
    assert!(body["tools"][0].get("function").is_none());
}

#[test]
fn asking_for_effort_turns_thinking_on_and_never_sends_a_budget() {
    let messages = vec![message(Role::User, "hi")];
    let mut call = ask(&messages);
    call.effort = Some(Effort::High);
    let body = request(&call, true);
    assert_eq!(body["output_config"]["effort"], "high");
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert!(
        body["thinking"].get("budget_tokens").is_none(),
        "budget_tokens is rejected by current models"
    );
    assert_eq!(body["stream"], true);
}

#[test]
fn effort_none_asks_for_the_lowest_level_the_api_accepts() {
    let messages = vec![message(Role::User, "hi")];
    let mut call = ask(&messages);
    call.effort = Some(Effort::None);
    let body = request(&call, false);
    assert_eq!(
        body["output_config"]["effort"], "low",
        "there is no `none` effort on this API"
    );
}

#[test]
fn a_reply_parses_into_text_thinking_and_calls() {
    let body = json!({
        "id": "msg_1",
        "content": [
            {"type": "thinking", "thinking": "let me think"},
            {"type": "text", "text": "Checking."},
            {"type": "tool_use", "id": "toolu_1", "name": "get_weather",
             "input": {"location": "Paris"}}
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });
    let message = parse("claude-opus-5", &body);
    assert_eq!(message.content.as_deref(), Some("Checking."));
    assert_eq!(message.reasoning_content.as_deref(), Some("let me think"));
    assert_eq!(message.stop_reason, StopReason::ToolCalls);
    assert_eq!(message.tool_calls.len(), 1);
    assert_eq!(message.tool_calls[0].id, "toolu_1");
    let arguments: Value = serde_json::from_str(&message.tool_calls[0].function.arguments).unwrap();
    assert_eq!(arguments["location"], "Paris");
}

#[test]
fn cache_tokens_land_in_their_own_counter() {
    let counted = usage(Some(&json!({
        "input_tokens": 100,
        "cache_creation_input_tokens": 20,
        "cache_read_input_tokens": 300,
        "output_tokens": 7
    })));
    assert_eq!(
        counted.prompt_tokens, 120,
        "writing the cache is charged as input"
    );
    assert_eq!(
        counted.cached_tokens, 300,
        "a cache read is counted apart from fresh input"
    );
    assert_eq!(counted.completion_tokens, 7);
}

#[test]
fn every_stop_reason_maps_to_one_of_ours() {
    assert_eq!(stop_reason(Some("tool_use")), StopReason::ToolCalls);
    assert_eq!(stop_reason(Some("max_tokens")), StopReason::Length);
    assert_eq!(stop_reason(Some("end_turn")), StopReason::Stop);
    assert_eq!(stop_reason(None), StopReason::Stop);
}
