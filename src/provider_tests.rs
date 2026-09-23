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
