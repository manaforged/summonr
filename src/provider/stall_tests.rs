use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use super::*;
use crate::error::{Error, ProviderFailureKind};

fn chunk(data: &str) -> String {
    format!("{:x}\r\n{data}\r\n", data.len())
}

fn stalled_server(filler: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0u8; 65536];
        drop(socket.read(&mut request));
        let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
        let first = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
        if socket
            .write_all(format!("{head}{}", chunk(first)).as_bytes())
            .is_err()
        {
            return;
        }
        drop(socket.set_read_timeout(Some(Duration::from_millis(100))));
        for _ in 0..600 {
            drop(socket.read(&mut request));
            if socket.write_all(chunk(filler).as_bytes()).is_err() {
                return;
            }
        }
    });
    base
}

async fn cut(filler: &'static str) -> (crate::error::ProviderFailure, Partial) {
    let mut client = Client::new("k", stalled_server(filler)).unwrap();
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
