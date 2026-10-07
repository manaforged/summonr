use serde_json::Value;

use super::keys::builtin;
use crate::provider::{Client, CompletionRequest, Effort, Message};

fn body(name: &str, model: &str, effort: Option<Effort>) -> Value {
    let mut endpoint = builtin(name).unwrap();
    endpoint.model = model.to_string();
    endpoint.api_key = Some("k".to_string());
    let client = Client::connect(&endpoint).unwrap();
    let messages = [Message::user("hi".to_string())];
    let request = CompletionRequest::new(
        "s".to_string(),
        model.to_string(),
        &messages,
        &[],
        Some(16_000),
        false,
        effort,
    );
    client.request_body(&request, false).unwrap()
}

#[test]
fn deepseek_turns_thinking_off_with_its_switch_not_an_effort_value() {
    let sent = body("deepseek", "deepseek-flash", Some(Effort::None));
    assert_eq!(sent["thinking"]["type"], "disabled");
    assert!(sent.get("reasoning_effort").is_none(), "{sent}");
}

#[test]
fn deepseek_gets_only_a_level_it_documents() {
    assert_eq!(
        body("deepseek", "deepseek-flash", Some(Effort::Medium))["reasoning_effort"],
        "low"
    );
    assert_eq!(
        body("deepseek", "deepseek-flash", Some(Effort::Max))["reasoning_effort"],
        "max"
    );
}

#[test]
fn glm_5_3_flash_clamps_to_its_three_levels_and_cannot_turn_thinking_off() {
    assert_eq!(
        body("zai", "glm-5.3-flash", Some(Effort::High))["reasoning_effort"],
        "high"
    );
    let off = body("zai", "glm-5.3-flash", Some(Effort::None));
    assert_eq!(off["reasoning_effort"], "low");
    assert_eq!(off["thinking"]["type"], "enabled", "{off}");
}

#[test]
fn openrouter_sends_effort_inside_a_reasoning_object() {
    let sent = body("openrouter", "openai/gpt-5.5", Some(Effort::XHigh));
    assert_eq!(sent["reasoning"]["effort"], "xhigh");
    assert!(sent.get("reasoning_effort").is_none(), "{sent}");
}

#[test]
fn qwen_plan_sends_the_documented_effort_field_and_nothing_for_none() {
    let off = body("qwen-plan", "qwen3-coder-next", Some(Effort::None));
    assert!(off.get("reasoning_effort").is_none(), "{off}");
    assert!(off.get("chat_template_kwargs").is_none(), "{off}");
    assert!(off.get("enable_thinking").is_none(), "{off}");
    let up = body("qwen-plan", "qwen3.5-plus", Some(Effort::High));
    assert_eq!(up["reasoning_effort"], "high");
}

#[test]
fn unverified_plan_hosts_send_no_effort_field_at_all() {
    for (name, model) in [
        ("tencent-plan", "hunyuan-2.0-thinking"),
        ("volcengine-plan", "doubao-seed-2.1-turbo"),
    ] {
        for effort in [None, Some(Effort::None), Some(Effort::High)] {
            let sent = body(name, model, effort);
            assert!(sent.get("reasoning_effort").is_none(), "{name}: {sent}");
            assert!(sent.get("thinking").is_none(), "{name}: {sent}");
            assert!(sent.get("chat_template_kwargs").is_none(), "{name}: {sent}");
        }
    }
}

#[test]
fn minimax_plan_sends_no_effort_field_when_none_is_asked_for() {
    let sent = body("minimax-plan", "MiniMax-M3", Some(Effort::None));
    assert!(sent.get("reasoning_effort").is_none(), "{sent}");
    assert!(sent.get("thinking").is_none(), "{sent}");
}

#[test]
fn grok_never_gets_none() {
    assert_eq!(
        body("grok", "grok-4.5", Some(Effort::None))["reasoning_effort"],
        "low"
    );
}

#[test]
fn anthropic_levels_follow_the_model() {
    let opus = body("anthropic", "claude-opus-4-8", Some(Effort::XHigh));
    assert_eq!(opus["output_config"]["effort"], "xhigh");
    let older = body("anthropic", "claude-opus-4-6", Some(Effort::XHigh));
    assert_eq!(older["output_config"]["effort"], "high");
}

#[test]
fn a_model_without_reasoning_gets_no_effort_field() {
    let sent = body("openai", "gpt-4o", Some(Effort::High));
    assert!(sent.get("reasoning_effort").is_none(), "{sent}");
    assert!(sent.get("reasoning").is_none(), "{sent}");
}

#[test]
fn gemini_3_pro_gets_one_of_its_two_levels() {
    let sent = body("gemini", "gemini-3-pro-preview", Some(Effort::Medium));
    assert_eq!(
        sent["generationConfig"]["thinkingConfig"]["thinkingLevel"],
        "low"
    );
}

fn replayed_reasoning(name: &str, model: &str) -> Value {
    replayed_through(name, model, None)
}

fn replayed_through(name: &str, model: &str, proxy: Option<&str>) -> Value {
    let mut endpoint = builtin(name).unwrap();
    if let Some(proxy) = proxy {
        endpoint.name = super::ENV_ENDPOINT.to_string();
        endpoint.base_url = Some(proxy.to_string());
        endpoint.keep_reasoning = false;
    }
    endpoint.model = model.to_string();
    endpoint.api_key = Some("k".to_string());
    let client = Client::connect(&endpoint).unwrap();
    let assistant = Message {
        role: crate::provider::Role::Assistant,
        content: "checking".to_string(),
        reasoning: Some("my earlier chain of thought".to_string()),
        ..Message::user(String::new())
    };
    let messages = [
        Message::user("hi".to_string()),
        assistant,
        Message::user("go on".to_string()),
    ];
    let request = CompletionRequest::new(
        "s".to_string(),
        model.to_string(),
        &messages,
        &[],
        Some(16_000),
        false,
        None,
    );
    let body = client.request_body(&request, false).unwrap();
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "assistant")
        .unwrap()["reasoning_content"]
        .clone()
}

#[test]
fn deepseek_and_glm_get_their_earlier_reasoning_back() {
    assert_eq!(
        replayed_reasoning("deepseek", "deepseek-flash"),
        "my earlier chain of thought"
    );
    assert_eq!(
        replayed_reasoning("zai", "glm-5.3-flash"),
        "my earlier chain of thought"
    );
}

#[test]
fn deepseek_and_glm_get_their_reasoning_back_behind_a_proxy() {
    let proxy = Some("http://172.17.0.1:8095/label/v1");
    assert_eq!(
        replayed_through("deepseek", "deepseek-flash", proxy),
        "my earlier chain of thought"
    );
    assert_eq!(
        replayed_through("zai", "glm-5.3-flash", proxy),
        "my earlier chain of thought"
    );
}

#[test]
fn a_vendor_that_does_not_ask_for_it_gets_no_replayed_reasoning() {
    assert_eq!(
        replayed_reasoning("groq", "openai/gpt-oss-120b"),
        Value::Null
    );
}

#[test]
fn a_tool_image_reaches_a_chat_completions_model_as_a_user_attachment() {
    let mut endpoint = builtin("zai").unwrap();
    endpoint.model = "glm-5.3-flash".to_string();
    endpoint.api_key = Some("k".to_string());
    let client = Client::connect(&endpoint).unwrap();
    let assistant = Message {
        role: crate::provider::Role::Assistant,
        tool_calls: vec![crate::provider::ToolCall {
            id: "t1".to_string(),
            kind: crate::provider::ToolKind::Function,
            function: crate::provider::ToolFunction {
                checked: Default::default(),
                name: "read".to_string(),
                arguments: "{}".to_string(),
            },
            continuation: None,
        }],
        ..Message::user(String::new())
    };
    let result = Message {
        images: vec![crate::provider::Image::encode("image/png", &[1, 2, 3])],
        ..Message::tool("t1", "board.png is attached")
    };
    let messages = [Message::user("look".to_string()), assistant, result];
    let request = CompletionRequest::new(
        "s".to_string(),
        endpoint.model.clone(),
        &messages,
        &[],
        Some(16_000),
        false,
        None,
    );
    let body = client.request_body(&request, false).unwrap();
    let sent = body["messages"].as_array().unwrap();
    let tool = sent.iter().find(|m| m["role"] == "tool").unwrap();
    assert_eq!(tool["content"], "board.png is attached");
    let last = sent.last().unwrap();
    assert_eq!(last["role"], "user");
    assert_eq!(last["content"][1]["type"], "image_url", "{body}");
}

#[test]
fn glm_keeps_its_reasoning_and_streams_tool_arguments() {
    let mut endpoint = builtin("zai").unwrap();
    endpoint.name = super::ENV_ENDPOINT.to_string();
    endpoint.base_url = Some("http://172.17.0.1:8095/label/v1".to_string());
    endpoint.model = "glm-5.3-flash".to_string();
    endpoint.api_key = Some("k".to_string());
    let client = Client::connect(&endpoint).unwrap();
    let messages = [Message::user("hi".to_string())];
    let tools = [crate::provider::ToolDefinition {
        kind: crate::provider::ToolKind::Function,
        function: crate::provider::FunctionDefinition {
            name: "read".to_string(),
            description: "Read".to_string(),
            parameters: serde_json::json!({"type": "object"}),
        },
    }];
    let request = CompletionRequest::new(
        "s".to_string(),
        endpoint.model.clone(),
        &messages,
        &tools,
        Some(16_000),
        false,
        Some(Effort::High),
    );
    let sent = client.request_body(&request, true).unwrap();
    assert_eq!(
        sent["thinking"],
        serde_json::json!({"type": "enabled", "clear_thinking": false}),
        "{sent}"
    );
    assert_eq!(sent["tool_stream"], true, "{sent}");
}
