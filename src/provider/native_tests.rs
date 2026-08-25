use super::*;
use crate::provider::{FunctionDefinition, ToolDefinition, ToolKind};

#[test]
fn chat_options_use_the_harness_token_cap() {
    let options = chat_options(&[], 8192);
    assert_eq!(options.max_tokens, Some(8192));
}

#[test]
fn native_kind_binds_anthropic_and_gemini_only() {
    assert_eq!(
        Client::native_kind(Wire::Anthropic),
        Some(AdapterKind::Anthropic)
    );
    assert_eq!(Client::native_kind(Wire::Gemini), Some(AdapterKind::Gemini));
    assert_eq!(Client::native_kind(Wire::Completions), None);
}

#[test]
fn to_chat_lifts_system_and_tools() -> Result<()> {
    let request = CompletionRequest {
        model: "claude-sonnet-4-5".into(),
        messages: vec![
            crate::provider::Message::system("be brief"),
            crate::provider::Message::user("hello"),
        ],
        tools: vec![ToolDefinition {
            kind: ToolKind::Function,
            function: FunctionDefinition {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }],
        max_output_tokens: 128,
        emit_deltas: true,
    };
    let chat = to_chat(&request)?;
    assert_eq!(chat.system.as_deref(), Some("be brief"));
    assert_eq!(chat.messages.len(), 1);
    let tools = chat.tools.expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name.as_str(), "read_file");
    Ok(())
}
