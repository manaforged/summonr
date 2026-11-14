use std::io::{Read, Write};

use serde_json::Value;

use super::{Client, CompletionRequest, Effort, Message, Partial};
use crate::endpoint::Endpoint;

/// `endpoint` with a placeholder API key, so a client connects without a stored credential.
pub(crate) fn keyed(mut endpoint: Endpoint) -> Endpoint {
    endpoint.api_key = Some(summonr_core::secret::Secret::new("k"));
    endpoint
}

/// The named endpoint at `model`, keyed.
pub(crate) fn endpoint(name: &str, model: &str) -> Endpoint {
    keyed(crate::endpoint::resolve(Some(name), Some(model), crate::Requirement::Config).unwrap())
}

/// The JSON body a client for `endpoint` builds for `messages`, capped at `max` output tokens, at `effort`.
pub(crate) fn request_body(
    endpoint: &Endpoint,
    messages: &[Message],
    max: Option<u32>,
    effort: Option<Effort>,
) -> Value {
    let request = request(&endpoint.model, messages, max, effort);
    Client::connect(endpoint)
        .expect("the endpoint fixture creates a client")
        .request_body(&request, false)
        .expect("the endpoint request serializes")
}

/// The `request_body` for `messages` with 16,000 output tokens, at `effort`.
pub(crate) fn body(endpoint: &Endpoint, messages: &[Message], effort: Option<Effort>) -> Value {
    request_body(endpoint, messages, Some(16_000), effort)
}

/// A session `s` request to `model` for `messages`, capped at `max` output tokens, at `effort`.
pub(crate) fn request<'a>(
    model: &str,
    messages: &'a [Message],
    max: Option<u32>,
    effort: Option<Effort>,
) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        model.to_string(),
        messages,
        &[],
        max,
        false,
        effort,
    )
}

/// The lowercased HTTP request a client for `endpoint` sends for `request`, read by a local server that answers 400.
pub(crate) fn sent(mut endpoint: Endpoint, request: &CompletionRequest<'_>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    endpoint.base_url = Some(format!("http://{}/v1", listener.local_addr().unwrap()));
    endpoint.oauth = None;
    let heard = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            let read = socket.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            seen.extend_from_slice(&chunk[..read]);
            let Some(end) = seen.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if seen.len() >= end + 4 + length {
                break;
            }
        }
        socket
            .write_all(
                b"HTTP/1.1 400 Bad Request\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
            )
            .unwrap();
        String::from_utf8_lossy(&seen).to_ascii_lowercase()
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _context = runtime.enter();
    let client = Client::connect(&endpoint).unwrap();
    let outcome = runtime.block_on(crate::provider::Provider::stream(
        &client,
        request,
        &mut Partial::default(),
    ));
    let failure = outcome.unwrap_err().to_string();
    let seen = heard.join().unwrap();
    assert!(!seen.is_empty(), "no request reached the server: {failure}");
    seen
}
