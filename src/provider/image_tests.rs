use super::*;
use crate::endpoint::Endpoint;

fn endpoint(name: &str, model: &str, images: Option<bool>) -> Endpoint {
    let mut endpoint =
        crate::endpoint::resolve(Some(name), Some(model), crate::Need::Config).unwrap();
    endpoint.api_key = Some("k".to_string());
    if images.is_some() {
        endpoint.images = images;
    }
    endpoint
}

fn body(endpoint: &Endpoint, messages: &[Message]) -> String {
    let request = CompletionRequest::new(
        "s".to_string(),
        endpoint.model.clone(),
        messages,
        &[],
        None,
        false,
        None,
    );
    Client::connect(endpoint)
        .unwrap()
        .request_body(&request, false)
        .unwrap()
        .to_string()
}

fn pictured(text: &str, images: Vec<Image>) -> Message {
    Message::user(UserInput {
        text: text.to_string(),
        images,
        source: None,
    })
}

fn png(bytes: usize) -> Image {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.resize(bytes, 0);
    Image::encode("image/png", &data)
}

fn tool_round(images: Vec<Image>) -> Vec<Message> {
    let mut call = Message::user("");
    call.role = Role::Assistant;
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
    let mut result = Message::user("shot.png is attached");
    result.role = Role::Tool;
    result.tool_call_id = Some("call_1".to_string());
    result.images = images;
    vec![call, result]
}

#[test]
fn history_images_never_reach_a_text_only_glm_model() {
    let glm = endpoint("glm-intl", "glm-5.3", None);
    let mut messages = vec![
        Message::system("sys"),
        pictured("here is a pic of me", vec![png(64)]),
    ];
    messages.extend(tool_round(vec![png(64)]));
    messages.push(Message::user("what now?"));
    let sent = body(&glm, &messages);
    assert!(!sent.contains("image_url"), "{sent}");
    assert_eq!(sent.matches("[image omitted: ").count(), 2, "{sent}");
}

#[test]
fn an_anthropic_request_keeps_only_the_newest_hundred_images() {
    let claude = endpoint("anthropic", "claude-opus-5-5", None);
    let messages: Vec<Message> = (0..101)
        .map(|turn| pictured(&format!("shot {turn}"), vec![png(64)]))
        .collect();
    let sent = body(&claude, &messages);
    assert_eq!(sent.matches(r#""type":"image""#).count(), 100);
    let first = sent.find("shot 0").unwrap();
    let omitted = sent.find("[image omitted: ").unwrap();
    assert!(omitted > first && omitted < sent.find("shot 1").unwrap());
}

#[test]
fn a_gemini_request_stays_under_its_inline_byte_limit() {
    let gemini = endpoint("gemini", "gemini-2.5-flash", None);
    let messages: Vec<Message> = (0..3)
        .map(|turn| pictured(&format!("shot {turn}"), vec![png(5 * 1024 * 1024)]))
        .collect();
    let sent = body(&gemini, &messages);
    assert_eq!(sent.matches("inlineData").count(), 2);
    assert!(sent.len() < 20 * 1000 * 1000, "{}", sent.len());
}

#[test]
fn a_gif_never_reaches_grok() {
    let grok = endpoint("grok", "grok-4.6", Some(true));
    let gif = Image::encode("image/gif", b"GIF89a....");
    let sent = body(&grok, &[pictured("look", vec![gif, png(64)])]);
    assert_eq!(sent.matches(r#""type":"image_url""#).count(), 1, "{sent}");
    assert!(!sent.contains("data:image/gif"), "{sent}");
    assert!(sent.contains("[image omitted: "), "{sent}");
}

#[test]
fn an_anthropic_image_over_ten_megabytes_is_omitted() {
    let claude = endpoint("anthropic", "claude-opus-5-5", None);
    let sent = body(
        &claude,
        &[
            pictured("huge", vec![png(8 * 1024 * 1024)]),
            pictured("small", vec![png(64)]),
        ],
    );
    assert_eq!(sent.matches(r#""type":"image""#).count(), 1);
    assert!(
        sent.contains(r"huge\n\n[image omitted: larger than"),
        "{}",
        &sent[..200]
    );
}

#[test]
fn only_gemini_3_takes_a_tool_result_image_inside_the_function_response() {
    let mut messages = vec![Message::user("look")];
    messages.extend(tool_round(vec![png(64)]));
    let three = body(&endpoint("gemini", "gemini-3-flash", Some(true)), &messages);
    let three: serde_json::Value = serde_json::from_str(&three).unwrap();
    let response = &three["contents"][2]["parts"][0]["functionResponse"];
    assert_eq!(
        response["parts"][0]["inlineData"]["mimeType"], "image/png",
        "{three}"
    );
    let older = body(
        &endpoint("gemini", "gemini-2.5-flash", Some(true)),
        &messages,
    );
    assert!(!older.contains("inlineData"), "{older}");
    assert!(older.contains("[image omitted: "), "{older}");
}

#[test]
fn a_heic_tool_result_never_reaches_a_gemini_function_response() {
    let heic = Image::encode("image/heic", b"....ftypheic");
    let mut messages = vec![Message::user("look")];
    messages.extend(tool_round(vec![heic]));
    let sent = body(
        &endpoint("gemini", "gemini-3.8-flash", Some(true)),
        &messages,
    );
    assert!(!sent.contains("inlineData"), "{sent}");
    assert!(sent.contains("[image omitted: "), "{sent}");
    let shown = body(
        &endpoint("gemini", "gemini-3.8-flash", Some(true)),
        &[pictured(
            "look",
            vec![Image::encode("image/heic", b"....ftypheic")],
        )],
    );
    assert!(
        shown.contains("image/heic"),
        "a user image may still be HEIC: {shown}"
    );
}

#[test]
fn gemini_at_default_effort_asks_for_thought_summaries() {
    let sent = body(
        &endpoint("gemini", "gemini-3.8-flash", None),
        &[Message::user("hi")],
    );
    let sent: serde_json::Value = serde_json::from_str(&sent).unwrap();
    assert_eq!(
        sent["generationConfig"]["thinkingConfig"]["includeThoughts"], true,
        "{sent}"
    );
}
