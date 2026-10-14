use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

use super::*;
use crate::error::{Error, ProviderFailureKind, Result};

fn chunk(data: &str) -> String {
    format!("{:x}\r\n{data}\r\n", data.len())
}

const GREETING: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";

fn stalled_server(filler: &'static str) -> String {
    server(GREETING, filler, 600, "")
}

fn server(first: &'static str, filler: &'static str, rounds: usize, last: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        read_request(&mut socket);
        let mut request = [0u8; 65536];
        let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
        if socket
            .write_all(format!("{head}{}", chunk(first)).as_bytes())
            .is_err()
        {
            return;
        }
        drop(socket.set_read_timeout(Some(Duration::from_millis(100))));
        for _ in 0..rounds {
            drop(socket.read(&mut request));
            if socket.write_all(chunk(filler).as_bytes()).is_err() {
                return;
            }
        }
        drop(socket.write_all(format!("{}0\r\n\r\n", chunk(last)).as_bytes()));
    });
    base
}

async fn run(base: String) -> (Result<AssistantMessage>, Partial) {
    run_on(base, crate::endpoint::Wire::Completions).await
}

async fn run_on(base: String, wire: crate::endpoint::Wire) -> (Result<AssistantMessage>, Partial) {
    let mut client = Client::new("k", base).unwrap();
    client.wire = wire;
    client.idle = Some(Duration::from_secs(1));
    let messages = [Message::user("hi".to_string())];
    let request = CompletionRequest::new(
        String::new(),
        "m".to_string(),
        &messages,
        &[],
        None,
        true,
        None,
    );
    let mut partial = Partial::default();
    let outcome = tokio::time::timeout(
        Duration::from_secs(10),
        client.complete_stream(&request, &mut partial),
    )
    .await
    .expect("a stream that produces nothing must end, not hang");
    (outcome, partial)
}

async fn cut(filler: &'static str) -> (crate::error::ProviderFailure, Partial) {
    let (outcome, partial) = run(stalled_server(filler)).await;
    let Err(Error::Provider(failure)) = outcome else {
        panic!("expected a provider failure, got {outcome:?}");
    };
    (failure, partial)
}

#[tokio::test]
async fn a_stream_of_only_keep_alives_fails_as_a_retryable_cut() {
    let (failure, partial) = cut(": keep-alive\n\n").await;
    assert_eq!(failure.kind, ProviderFailureKind::Transport);
    assert!(
        failure.partial_output && failure.status.is_none(),
        "{failure:?}"
    );
    assert_eq!(partial.text, "hi");
}

#[tokio::test]
async fn a_stream_of_empty_deltas_fails_as_a_retryable_cut() {
    let (failure, partial) = cut("data: {\"choices\":[{\"delta\":{}}]}\n\n").await;
    assert_eq!(failure.kind, ProviderFailureKind::Transport);
    assert!(
        failure.partial_output && failure.status.is_none(),
        "{failure:?}"
    );
    assert_eq!(partial.text, "hi");
}

#[tokio::test]
async fn a_long_tool_argument_stream_is_not_cut_as_silence() {
    let first = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"write\",\"arguments\":\"\"}}]}}]}\n\n";
    let filler = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" \"}}]}}]}\n\n";
    let last = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
    let (outcome, _) = run(server(first, filler, 30, last)).await;
    let message = outcome.expect("argument fragments are progress, not silence");
    assert_eq!(message.tool_calls.len(), 1);
    assert_eq!(message.tool_calls[0].function.name, "write");
    assert!(message.tool_calls[0].function.arguments.ends_with("{}"));
}

#[tokio::test]
async fn a_long_silent_reasoning_stretch_is_not_cut_as_silence() {
    let first = "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"reasoning\",\"id\":\"rs_0\",\"summary\":[]}}\n\n";
    let filler = "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"reasoning\",\"id\":\"rs_0\",\"summary\":[]}}\n\ndata: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"reasoning\",\"id\":\"rs_1\",\"summary\":[]}}\n\n";
    let last = "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"message\",\"id\":\"msg_0\",\"role\":\"assistant\",\"content\":[]}}\n\ndata: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_0\",\"delta\":\"done\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[]}}\n\n";
    let (outcome, _) = run_on(
        server(first, filler, 30, last),
        crate::endpoint::Wire::Responses,
    )
    .await;
    let message = outcome.expect("reasoning item frames are progress, not silence");
    assert_eq!(message.content.as_deref(), Some("done"));
}

#[tokio::test]
async fn a_done_sentinel_without_a_finish_reason_is_not_a_finished_reply() {
    let last = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":null}]}\n\ndata: [DONE]\n\n";
    let (outcome, partial) = run(server(GREETING, GREETING, 1, last)).await;
    let Err(Error::Provider(failure)) = outcome else {
        panic!("expected a provider failure, got {outcome:?}");
    };
    assert_eq!(failure.kind, ProviderFailureKind::Transport);
    assert_eq!(partial.text, "hihi");
}

fn dropped(first: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        read_request(&mut socket);
        let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
        drop(socket.write_all(format!("{head}{}", chunk(first)).as_bytes()));
        drop(socket.shutdown(std::net::Shutdown::Both));
    });
    base
}

#[tokio::test(flavor = "multi_thread")]
async fn a_finished_reply_survives_a_dropped_connection() {
    let finished =
        "data: {\"choices\":[{\"delta\":{\"content\":\"done\"},\"finish_reason\":\"stop\"}]}\n\n";
    let (reply, _) = run(dropped(finished)).await;
    let reply = reply.expect("a finished reply is not a cut");
    assert_eq!(reply.content.as_deref(), Some("done"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cut_reply_keeps_the_usage_already_reported() {
    let counted = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}],\"usage\":{\"prompt_tokens\":150000,\"completion_tokens\":3}}\n\n";
    let (reply, partial) = run(dropped(counted)).await;
    assert!(reply.is_err(), "{reply:?}");
    assert_eq!(
        partial.usage.map(|usage| usage.prompt_tokens),
        Some(150_000)
    );
}

fn read_request(socket: &mut TcpStream) {
    let mut request = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        let Ok(read) = socket.read(&mut buf) else {
            return;
        };
        if read == 0 {
            return;
        }
        request.extend_from_slice(&buf[..read]);
        let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if request.len() >= end + 4 + length {
            return;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_cut_at_the_output_limit_before_any_text_is_a_length_stop() {
    let cut =
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n";
    let (reply, _) = run(server(cut, "", 0, "")).await;
    assert_eq!(reply.unwrap().stop_reason, StopReason::Length);
}
