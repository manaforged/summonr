use serde_json::json;

use super::Message;
use super::cache_tests::{body, endpoint, plain, sent};

#[test]
fn each_host_gets_the_session_cache_hint_its_docs_name() {
    let messages = [Message::system("sys"), Message::user("hi")];
    let openrouter = sent(
        endpoint("openrouter", "openai/gpt-4o"),
        &plain("openai/gpt-4o", &messages, None),
    );
    assert!(openrouter.contains("x-session-id: s\r\n"), "{openrouter}");
    let fireworks = sent(
        endpoint("fireworks", "accounts/fireworks/models/kimi-k3"),
        &plain("accounts/fireworks/models/kimi-k3", &messages, None),
    );
    assert!(
        fireworks.contains("x-session-affinity: s\r\n"),
        "{fireworks}"
    );
    let mistral = body(&endpoint("mistral", "devstral-latest"), &messages, None);
    assert_eq!(mistral["prompt_cache_key"], "s", "{mistral}");
    let dashscope = body(&endpoint("dashscope", "qwen3.7-plus"), &messages, None);
    assert_eq!(
        dashscope["messages"][0]["content"],
        json!([{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}])
    );
    let start = crate::endpoint::start_command("http://127.0.0.1:11434/v1").unwrap();
    assert!(start.contains("OLLAMA_CONTEXT_LENGTH="), "{start}");
}
