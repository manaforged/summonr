use super::*;
use crate::provider::Effort;

use crate::provider::{FunctionDefinition, Image, ToolDefinition};

pub(super) fn message(role: Role, content: &str) -> Message {
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

pub(super) fn call(id: &str, name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: name.to_string(),
            arguments: arguments.to_string(),
            checked: Default::default(),
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
    )
    .unwrap();
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
    let body = request(&ask(&[message(Role::User, "weather?"), result]), false).unwrap();
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
    let body = request(&ask(&[message(Role::User, "go"), first, second]), false).unwrap();
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
    let body = request(&ask(&[message(Role::User, "weather?"), assistant]), false).unwrap();
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
    let body = request(&ask(&[user]), false).unwrap();
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
    let body = request(&call, false).unwrap();
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
    let body = request(&call, true).unwrap();
    assert_eq!(body["output_config"]["effort"], "high");
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert!(
        body["thinking"].get("budget_tokens").is_none(),
        "budget_tokens is rejected by current models"
    );
    assert_eq!(body["stream"], true);
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
fn every_stop_reason_maps_to_one_of_ours() {
    assert_eq!(stop_reason(Some("tool_use")), StopReason::ToolCalls);
    assert_eq!(stop_reason(Some("max_tokens")), StopReason::Length);
    assert_eq!(stop_reason(Some("end_turn")), StopReason::Stop);
    assert_eq!(stop_reason(None), StopReason::Stop);
}

fn asked<'a>(model: &str, messages: &'a [Message], effort: Effort) -> CompletionRequest<'a> {
    let mut call = ask(messages).with_model(model.to_string());
    call.effort = Some(effort);
    call.max_output_tokens = Some(16_000);
    call
}

#[test]
fn the_request_path_sits_under_the_versioned_base_url() {
    assert_eq!(
        <Wire as crate::provider::wire::Chat>::path("claude-sonnet-5", true),
        "/messages",
        "the preset base_url already ends in /v1"
    );
}

#[test]
fn a_model_before_adaptive_thinking_gets_a_budget_below_max_tokens() {
    let messages = vec![message(Role::User, "hi")];
    let body = request(&asked("claude-sonnet-4-5", &messages, Effort::High), false).unwrap();
    assert_eq!(body["thinking"]["type"], "enabled");
    let budget = body["thinking"]["budget_tokens"].as_u64().unwrap();
    assert!((1024..1024 * 16).contains(&budget), "got {budget}");
    assert!(budget < body["max_tokens"].as_u64().unwrap());
    assert!(body.get("output_config").is_none());
}

#[test]
fn a_model_outside_both_lists_gets_no_thinking() {
    let messages = vec![message(Role::User, "hi")];
    let body = request(&asked("glm-5.3", &messages, Effort::High), false).unwrap();
    assert!(body.get("thinking").is_none());
    assert!(body.get("output_config").is_none());
}

#[test]
fn effort_none_disables_thinking_on_sonnet_5() {
    let messages = vec![message(Role::User, "hi")];
    let body = request(&asked("claude-sonnet-5", &messages, Effort::None), false).unwrap();
    assert_eq!(body["thinking"]["type"], "disabled");
    assert!(body.get("output_config").is_none());
}

#[test]
fn a_failed_tool_result_is_marked_as_an_error() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![call("t1", "read", "{}")];
    let mut result = message(Role::Tool, "no such file");
    result.tool_call_id = Some("t1".to_string());
    result.failed = true;
    let messages = vec![message(Role::User, "read it"), assistant, result];
    let body = request(&ask(&messages), false).unwrap();
    let block = &body["messages"][2]["content"][0];
    assert_eq!(block["type"], "tool_result");
    assert_eq!(block["is_error"], true);
}

#[test]
fn a_tool_result_carries_its_images() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![call("t1", "show_image", "{}")];
    let mut result = message(Role::Tool, "here");
    result.tool_call_id = Some("t1".to_string());
    result.images = vec![Image::encode("image/png", &[1, 2, 3])];
    let messages = vec![message(Role::User, "show"), assistant, result];
    let body = request(&ask(&messages), false).unwrap();
    let content = &body["messages"][2]["content"][0]["content"];
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[1]["type"], "text");
}

#[test]
fn an_assistant_turn_with_nothing_to_say_is_left_out() {
    let messages = vec![
        message(Role::User, "hi"),
        message(Role::Assistant, ""),
        message(Role::User, "continue"),
    ];
    let body = request(&ask(&messages), false).unwrap();
    let roles: Vec<&str> = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|turn| turn["role"].as_str().unwrap())
        .collect();
    assert!(!roles.contains(&"assistant"), "got {roles:?}");
}

#[test]
fn a_note_between_a_call_and_its_result_comes_after_the_result() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![call("t1", "read", "{}")];
    let mut result = message(Role::Tool, "ok");
    result.tool_call_id = Some("t1".to_string());
    let messages = vec![
        message(Role::User, "go"),
        assistant,
        message(Role::System, "plan note"),
        result,
    ];
    let body = request(&ask(&messages), false).unwrap();
    let parts = body["messages"][2]["content"].as_array().unwrap();
    assert_eq!(parts[0]["type"], "tool_result");
    assert_eq!(parts[1]["text"], "plan note");
}

#[test]
fn replayed_tool_arguments_keep_the_bytes_the_model_produced() {
    let mut assistant = message(Role::Assistant, "");
    assistant.tool_calls = vec![call("toolu_1", "lookup", r#"{"zeta": 1.50, "alpha":"x"}"#)];
    let wired = <Wire as crate::provider::wire::Chat>::body(
        &ask(&[message(Role::User, "go"), assistant]),
        &Reasoning::default(),
        crate::endpoint::CacheTtl::default(),
        false,
    )
    .unwrap();
    let raw = String::from_utf8(wired.messages.expect("the history is written raw")).unwrap();
    assert!(
        raw.contains(r#""input":{"zeta": 1.50, "alpha":"x"}"#),
        "arguments are embedded verbatim, not re-serialized: {raw}"
    );
}

#[test]
fn a_cache_write_is_priced_at_the_write_rate() {
    let usage = super::usage(Some(&serde_json::json!({
        "input_tokens": 100,
        "cache_creation_input_tokens": 1000,
        "cache_read_input_tokens": 2000,
        "output_tokens": 50,
    })));
    assert_eq!(
        (
            usage.prompt_tokens,
            usage.cached_tokens,
            usage.cache_write_tokens
        ),
        (3100, 2000, 1000)
    );
    let price = crate::provider::Price {
        input: 3.0,
        output: 15.0,
        cached: 0.3,
        cache_write: Some(3.75),
    };
    let cost = price.cost(
        u64::from(usage.prompt_tokens),
        u64::from(usage.completion_tokens),
        u64::from(usage.cached_tokens),
        u64::from(usage.cache_write_tokens),
    );
    assert!((cost - 0.0054).abs() < 1e-12, "{cost}");
}
