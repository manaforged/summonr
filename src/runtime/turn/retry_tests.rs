use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};
use crate::provider::{AssistantMessage, CompletionRequest, Message, Partial, Provider};
use crate::runtime::Loop;

struct Fault {
    kind: ProviderFailureKind,
    partial: bool,
    fails: usize,
    sent: Mutex<Vec<Vec<Message>>>,
}

impl Provider for Fault {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move { self.stream(request, &mut Partial::default()).await })
    }

    fn stream<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
        partial: &'a mut Partial,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        let mut sent = self.sent.lock().expect("captured requests");
        sent.push(request.messages.to_vec());
        let failing = sent.len() <= self.fails;
        drop(sent);
        let reply = if failing {
            if self.partial {
                partial.text = "saved partial reply".to_string();
            }
            Err(Error::Provider(ProviderFailure {
                kind: self.kind,
                status: None,
                message: "fixture failure".to_string(),
                partial_output: self.partial,
                retry_after: (self.fails > 1).then_some(0),
            }))
        } else {
            Ok(AssistantMessage {
                content: Some("recovered".to_string()),
                ..AssistantMessage::default()
            })
        };
        Box::pin(async move { reply })
    }
}

async fn run(
    kind: ProviderFailureKind,
    partial: bool,
) -> (bool, usize, Vec<crate::durable::log::Line>) {
    run_failing(kind, partial, 1).await
}

async fn run_failing(
    kind: ProviderFailureKind,
    partial: bool,
    fails: usize,
) -> (bool, usize, Vec<crate::durable::log::Line>) {
    let workspace = tempfile::tempdir().expect("workspace");
    let home = tempfile::tempdir().expect("session home");
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .expect("open journal");
    let provider = Fault {
        kind,
        partial,
        fails,
        sent: Mutex::new(Vec::new()),
    };
    let mut loop_ =
        Loop::open(provider, "m", workspace.path(), "system", driver).expect("open runtime");
    let result = loop_.turn("hi there").await;
    let sent = loop_.provider.sent.lock().expect("captured requests");
    assert!(
        sent[0]
            .iter()
            .any(|message| message.content.contains("hi there"))
    );
    let lines = crate::durable::log::read(loop_.session_path()).expect("read durable result");
    (result.is_ok(), sent.len(), lines)
}

#[tokio::test]
async fn an_unchanged_context_overflow_stops_after_one_request() {
    let (ok, attempts, _) = run(ProviderFailureKind::ContextOverflow, false).await;
    assert!(!ok);
    assert_eq!(
        attempts, 1,
        "runtime must not resend the same overflowing context"
    );
}

#[tokio::test]
async fn transport_request_failures_do_not_get_an_extra_runtime_retry() {
    let (ok, attempts, _) = run(ProviderFailureKind::Transport, false).await;
    assert!(!ok);
    assert_eq!(attempts, 1, "request retries belong to the transport");
}

#[tokio::test]
async fn a_cut_stream_retries_once_and_preserves_interrupted_text() {
    let (ok, attempts, lines) = run(ProviderFailureKind::Transport, true).await;
    assert!(ok);
    assert_eq!(attempts, 2);
    assert!(lines.iter().any(|line| matches!(line, crate::durable::log::Line::Interrupted { text, .. } if text == "saved partial reply")), "{lines:?}");
    assert!(lines.iter().any(|line| matches!(line, crate::durable::log::Line::Assistant { text, .. } if text == "recovered")), "{lines:?}");
}

#[tokio::test]
async fn an_overloaded_stream_retries_up_to_the_transport_limit() {
    let limit = crate::limits::limits().retry_limit as usize;
    let (ok, attempts, _) = run_failing(ProviderFailureKind::Server, true, limit).await;
    assert!(
        ok,
        "a stream that keeps failing with a server error recovers within the limit"
    );
    assert_eq!(attempts, limit + 1);
    let (ok, attempts, _) = run_failing(ProviderFailureKind::Server, true, limit + 1).await;
    assert!(!ok);
    assert_eq!(attempts, limit + 1);
}
