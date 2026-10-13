use super::*;

fn request<'a>(messages: &'a [Message]) -> CompletionRequest<'a> {
    CompletionRequest::new(
        String::new(),
        "m".to_string(),
        messages,
        &[],
        None,
        false,
        None,
    )
}

#[test]
fn completions_keep_system_only_at_the_front() {
    let client = Client::new("k", "http://127.0.0.1/v1").unwrap();
    let body = client
        .request_body(
            &request(&[
                Message::system("sys"),
                Message::user("hi"),
                Message::system("receipt"),
                Message::user("next"),
            ]),
            false,
        )
        .unwrap();
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][0]["content"], "sys");
    assert_eq!(body["messages"][1]["role"], "user");
    assert_eq!(body["messages"][1]["content"], "hi");
    assert_eq!(body["messages"][2]["role"], "user");
    assert_eq!(body["messages"][2]["content"], "receipt");
    assert_eq!(body["messages"][3]["content"], "next");
}

#[test]
fn empty_tool_arguments_leave_as_a_json_object() {
    let client = Client::new("k", "http://127.0.0.1/v1").unwrap();
    let mut assistant = Message::user("");
    assistant.role = Role::Assistant;
    assistant.tool_calls = vec![ToolCall {
        id: "call_1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: String::new(),
        },
        continuation: None,
    }];
    let body = client.request_body(&request(&[assistant]), false).unwrap();
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["function"]["arguments"],
        "{}"
    );
}

#[test]
fn sglang_invalid_tool_json_is_malformed() {
    let error = response_failure(
        400,
        "Assistant tool call function.arguments must be valid JSON.",
        None,
    );
    assert_eq!(
        error.provider_failure().unwrap().kind,
        ProviderFailureKind::MalformedToolCall
    );
}

#[test]
fn sglang_input_length_400_is_overflow() {
    let text = "Input length (114895 tokens) exceeds the maximum allowed length (106234 tokens).";
    let error = response_failure(400, text, None);
    assert_eq!(
        error.provider_failure().unwrap().kind,
        ProviderFailureKind::ContextOverflow
    );
    assert_eq!(input_cap(text), Some(106_234));
}

#[test]
fn a_hosted_model_window_comes_from_local_data_without_a_request() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut endpoint =
        crate::endpoint::resolve(Some("zai"), Some("glm-5.3-flash"), crate::Need::Config).unwrap();
    endpoint.base_url = Some(format!("http://{}/v1", listener.local_addr().unwrap()));
    let client = Client::connect(&endpoint).unwrap();
    let window = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(client.context(&endpoint.name, &endpoint.model));
    assert!(window.is_some_and(|tokens| tokens > 0), "{window:?}");
    assert!(
        listener.accept().is_err(),
        "the window lookup reached the network"
    );
}

#[test]
fn a_client_lists_models_from_the_models_base_not_the_chat_base() {
    let mut endpoint =
        crate::endpoint::resolve(Some("deepseek"), None, crate::Need::Config).unwrap();
    endpoint.base_url = Some("https://api.perplexity.ai".to_string());
    endpoint.models_url = Some("https://api.perplexity.ai/v1".to_string());
    endpoint.api_key = Some("k".to_string());
    let client = Client::connect(&endpoint).unwrap();
    assert_eq!(client.base.as_str(), "https://api.perplexity.ai");
    assert_eq!(client.models_base.as_str(), "https://api.perplexity.ai/v1");
    endpoint.models_url = None;
    let plain = Client::connect(&endpoint).unwrap();
    assert_eq!(plain.models_base.as_str(), plain.base.as_str());
}

#[test]
fn a_plan_cap_is_a_rate_limit_that_names_its_code() {
    let text = r#"{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"cap"}}"#;
    let error = response_failure(429, text, None);
    let failure = error.provider_failure().unwrap();
    assert_eq!(failure.kind, ProviderFailureKind::RateLimited);
    assert!(
        failure
            .message
            .contains("subscription_sharing_usage_limit_exceeded"),
        "the user sees which plan rule stopped the turn, got {}",
        failure.message
    );
}

#[test]
fn an_ineligible_plan_user_is_not_asked_to_sign_in_again() {
    let text = r#"{"error":{"code":"subscription_sharing_user_not_eligible","message":"no"}}"#;
    let error = response_failure(403, text, None);
    assert_eq!(
        error.provider_failure().unwrap().kind,
        ProviderFailureKind::Http,
        "a 403 for eligibility must not read as a rejected key"
    );
}

#[test]
fn a_plan_cap_in_the_stream_keeps_its_message() {
    let error = crate::provider::wire::failed(
        None,
        "subscription_sharing_usage_limit_exceeded",
        "weekly cap reached",
    );
    let failure = error.provider_failure().unwrap();
    assert_eq!(failure.kind, ProviderFailureKind::RateLimited);
    assert!(
        failure.message.contains("weekly cap reached"),
        "the provider's words survive the stream path, got {}",
        failure.message
    );
}

#[test]
fn a_plan_permission_failure_does_not_read_as_a_rejected_key() {
    for code in [
        "chatpass_v2_scope_not_authorized",
        "chatpass_v2_invalid_authorization_context",
    ] {
        let text = format!(r#"{{"error":{{"code":"{code}","message":"no"}}}}"#);
        let error = response_failure(403, &text, None);
        assert_eq!(
            error.provider_failure().unwrap().kind,
            ProviderFailureKind::Http,
            "{code} must not start a login loop"
        );
    }
}

#[test]
fn a_rejected_key_error_does_not_echo_the_key() {
    let body = r#"{"error":{"message":"Incorrect API key provided: sk-ab12*****xy9z. You can find your API key at https://example.com.","type":"invalid_request_error"}}"#;
    let error = response_failure(401, body, None);
    let failure = error.provider_failure().unwrap();
    assert!(!failure.message.contains("xy9z"), "{}", failure.message);
    assert!(
        failure.message.contains("API key provided: [key]"),
        "{}",
        failure.message
    );
}

#[test]
fn a_refused_loopback_server_says_nothing_is_listening() {
    let base = "http://127.0.0.1:8771/v1";
    let refused = unreachable_message("lab", base, true, &"connect");
    assert!(
        refused.starts_with(
            "nothing is listening at 127.0.0.1:8771; start the server or fix the base_url"
        ),
        "{refused}"
    );
    let other = unreachable_message("lab", base, false, &"connect");
    assert!(other.contains("check the network"), "{other}");
    let remote = unreachable_message("x", "https://api.example.com/v1", true, &"connect");
    assert!(remote.contains("check the network"), "{remote}");
}
