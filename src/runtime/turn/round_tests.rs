use super::*;
use crate::provider::Image;
use crate::provider::scripted::{Reply, Scripted};

fn blind_session(workspace: &Path, home: &Path, provider: Scripted) -> Loop<Scripted> {
    let driver =
        super::super::super::configure::open_driver(home, workspace, "m", "system").unwrap();
    let mut loop_ = Loop::open(provider, "m", workspace, "system", driver).unwrap();
    loop_.set_images(false, None);
    loop_
}

fn picture(text: &str) -> UserInput {
    UserInput {
        text: text.to_string(),
        images: vec![Image::encode("image/png", b"not a real png")],
        source: None,
    }
}

fn assert_blind(sent: &[Vec<Message>]) {
    assert!(
        sent.iter()
            .flatten()
            .all(|message| message.images.is_empty()),
        "a blind model received image content: {sent:?}"
    );
}

#[tokio::test]
async fn an_image_on_a_blind_model_is_folded_to_text_not_sent() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let provider = Scripted::new([Reply::Text("ok")]);
    let sent = provider.sent();
    let mut loop_ = blind_session(workspace.path(), home.path(), provider);
    loop_
        .turn_with(picture("here is a pic of me"), |_| {})
        .await
        .unwrap();
    let sent = sent.lock().unwrap();
    assert_blind(&sent);
    assert!(
        sent.iter()
            .flatten()
            .any(|message| message.content.contains("image(s)")),
        "the fold note never reached the model: {sent:?}"
    );
}

#[tokio::test]
async fn a_switch_to_a_blind_model_folds_the_next_attached_image() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = blind_session(workspace.path(), home.path(), Scripted::new([]));
    loop_.set_images(true, None);
    let provider = Scripted::new([Reply::Text("ok")]);
    let sent = provider.sent();
    loop_.set_provider(provider, "glm-5.3");
    loop_
        .turn_with(picture("what is in this?"), |_| {})
        .await
        .unwrap();
    assert_blind(&sent.lock().unwrap());
}

#[tokio::test]
async fn a_fallback_to_a_blind_model_folds_the_next_attached_image() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let primary = Scripted::new([Reply::Fail("the primary is down")]);
    let mut loop_ = blind_session(workspace.path(), home.path(), primary);
    loop_.set_images(true, None);
    let fallback = Scripted::new([Reply::Text("ok"), Reply::Text("ok")]);
    let sent = fallback.sent();
    let second =
        crate::endpoint::resolve(Some("zai"), Some("glm-5.3"), crate::Need::Config).unwrap();
    loop_.set_fallbacks([(fallback, second)], None);
    loop_
        .turn_with(UserInput::from("hi"), |_| {})
        .await
        .unwrap();
    loop_
        .turn_with(picture("what is in this?"), |_| {})
        .await
        .unwrap();
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_blind(&sent);
}

fn two_reply_server(bodies: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let replies = [
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
        ];
        for reply in replies {
            let (mut socket, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut buffer = [0u8; 65536];
            loop {
                let read = socket.read(&mut buffer).unwrap();
                seen.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&seen);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if seen.len() >= end + 4 + length {
                        bodies.lock().unwrap().push(text[end + 4..].to_string());
                        break;
                    }
                }
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            );
            socket.write_all(head.as_bytes()).unwrap();
        }
    });
    base
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_reply_is_continued_once_instead_of_failing_the_turn() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let bodies = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let client = crate::provider::Client::new("k", two_reply_server(bodies.clone())).unwrap();
    let driver =
        super::super::super::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .unwrap();
    let mut loop_ = Loop::open(client, "m", workspace.path(), "system", driver).unwrap();
    let turn = loop_
        .turn_with(UserInput::from("hi"), |_| {})
        .await
        .unwrap();
    assert!(format!("{turn:?}").contains("done"), "{turn:?}");
    let bodies = bodies.lock().unwrap();
    assert_eq!(bodies.len(), 2);
    assert!(bodies[1].contains("Please continue"), "{}", bodies[1]);
}

#[tokio::test]
async fn a_fallback_takes_its_endpoint_settings_and_price() {
    use crate::provider::{Effort, Price};
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let primary = Scripted::new([Reply::Fail("the primary is down")]);
    let mut loop_ = blind_session(workspace.path(), home.path(), primary);
    loop_.set_price(Some(Price {
        input: 1.0,
        output: 1.0,
        cached: 0.0,
        cache_write: None,
    }));
    loop_.set_context_window(Some(262_144));
    loop_.set_compact_tokens(Some(200_000));
    loop_.set_effort(Some(Effort::Low));
    let mut second =
        crate::endpoint::resolve(Some("zai"), Some("glm-5.3"), crate::Need::Config).unwrap();
    let price = Price {
        input: 9.0,
        output: 9.0,
        cached: 0.0,
        cache_write: None,
    };
    second.price = Some(price);
    second.context_window = Some(4_096);
    second.compact_tokens = Some(3_000);
    second.effort = Some(Effort::High);
    let fallback = Scripted::new([Reply::Text("ok")]);
    loop_.set_fallbacks([(fallback, second)], None);
    loop_
        .turn_with(UserInput::from("hi"), |_| {})
        .await
        .unwrap();
    assert_eq!(loop_.model(), "glm-5.3");
    assert_eq!(loop_.price, Some(price));
    assert_eq!(loop_.context_window(), Some(4_096));
    assert_eq!(loop_.compact_tokens, Some(3_000));
    assert_eq!(loop_.effort(), Some(Effort::High));
}
