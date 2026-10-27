use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};
use crate::provider::{AssistantMessage, CompletionRequest, Partial, Provider};
use crate::runtime::Loop;

#[tokio::test]
async fn usage_exhaustion_stops_before_retry_or_fallback() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")?;
    let error = crate::provider::response_failure(
        429,
        r#"{"error":{"code":"insufficient_quota","message":"allowance exhausted"}}"#,
        Some(0),
    );
    let primary_sent = Arc::default();
    let fallback_sent = Arc::default();
    let primary = Fault {
        kind: error.provider_failure().expect("provider failure").kind,
        partial: false,
        fails: usize::MAX,
        asked: Some(0),
        sent: Arc::clone(&primary_sent),
        text: "fixture failure",
    };
    let fallback = Fault {
        kind: ProviderFailureKind::Http,
        partial: false,
        fails: 0,
        asked: None,
        sent: Arc::clone(&fallback_sent),
        text: "fixture failure",
    };
    let mut loop_ = Loop::open(primary, "m", workspace.path(), "system", driver)?;
    let endpoint = crate::endpoint::resolve(Some("openai"), Some("alias"), crate::Need::Config)?;
    loop_.set_fallbacks([(fallback, endpoint)]);
    let failure = loop_
        .turn("finish the kernel candidate")
        .await
        .expect_err("exhausted allowance stops the turn");
    assert_eq!(
        failure.provider_failure().expect("provider failure").kind,
        error.provider_failure().expect("classified allowance").kind
    );
    let sent = primary_sent.lock().expect("primary requests");
    assert_eq!(sent.len(), 1, "exhausted allowance was retried");
    assert!(
        sent[0]
            .iter()
            .any(|message| message.content.contains("finish the kernel candidate"))
    );
    assert!(
        fallback_sent.lock().expect("fallback requests").is_empty(),
        "exhausted allowance reached a fallback alias"
    );
    Ok(())
}

struct Fault {
    kind: ProviderFailureKind,
    partial: bool,
    fails: usize,
    asked: Option<u64>,
    sent: crate::provider::scripted::Sent,
    text: &'static str,
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
                message: self.text.to_string(),
                partial_output: self.partial,
                retry_after: self.asked.or((self.fails > 1).then_some(0)),
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
        asked: None,
        sent: Arc::default(),
        text: "fixture failure",
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

#[tokio::test]
async fn a_rate_limited_request_is_retried() {
    let (ok, attempts, _) = run_failing(ProviderFailureKind::RateLimited, false, 3).await;
    assert!(ok, "a rate limit that clears recovers the turn");
    assert_eq!(attempts, 4);
}

#[tokio::test]
async fn a_rate_limit_is_waited_out_past_the_retry_limit_while_the_time_budget_lasts() {
    let fails = crate::limits::limits().retry_limit as usize + 3;
    let workspace = tempfile::tempdir().expect("workspace");
    let home = tempfile::tempdir().expect("session home");
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .expect("open journal");
    let provider = Fault {
        kind: ProviderFailureKind::RateLimited,
        partial: false,
        fails,
        asked: None,
        sent: Arc::default(),
        text: "fixture failure",
    };
    let mut loop_ = Loop::open(provider, "m", workspace.path(), "system", driver)
        .expect("open runtime")
        .with_budget(crate::Budget {
            max_tokens: None,
            max_secs: Some(600),
        })
        .expect("budget");
    let result = loop_.turn("hi there").await;
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(
        loop_.provider.sent.lock().expect("requests").len(),
        fails + 1
    );
}

#[tokio::test(start_paused = true)]
async fn a_rate_limit_inside_a_time_budget_waits_the_full_time_the_provider_asks() {
    let workspace = tempfile::tempdir().expect("workspace");
    let home = tempfile::tempdir().expect("session home");
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .expect("open journal");
    let provider = Fault {
        kind: ProviderFailureKind::RateLimited,
        partial: false,
        fails: 1,
        asked: Some(120),
        sent: Arc::default(),
        text: "fixture failure",
    };
    let mut loop_ = Loop::open(provider, "m", workspace.path(), "system", driver)
        .expect("open runtime")
        .with_budget(crate::Budget {
            max_tokens: None,
            max_secs: Some(600),
        })
        .expect("budget");
    let started = tokio::time::Instant::now();
    let result = loop_.turn("hi there").await;
    assert!(result.is_ok(), "{result:?}");
    assert!(
        started.elapsed() >= std::time::Duration::from_secs(120),
        "retried after {:?}, before the 120 s the provider asked for",
        started.elapsed()
    );
}

#[tokio::test]
async fn an_overflow_cap_sticks_on_the_seated_model() {
    let workspace = tempfile::tempdir().expect("workspace");
    let home = tempfile::tempdir().expect("session home");
    let driver =
        crate::runtime::configure::open_driver(home.path(), workspace.path(), "m", "system")
            .expect("open journal");
    let idle = Fault {
        kind: ProviderFailureKind::Http,
        partial: false,
        fails: 0,
        asked: None,
        sent: Arc::default(),
        text: "fixture failure",
    };
    let seat = Fault {
        kind: ProviderFailureKind::ContextOverflow,
        partial: false,
        fails: 1,
        asked: None,
        sent: Arc::default(),
        text: "This model's maximum context length is 8000 tokens.",
    };
    let mut loop_ =
        Loop::open(idle, "m", workspace.path(), "system", driver).expect("open runtime");
    loop_.set_context_window(Some(1_000_000));
    loop_.set_seat(
        crate::workflow::Role::Implement,
        seat,
        "seat",
        None,
        None,
        None,
    );
    drop(loop_.turn("hi there").await);
    assert_eq!(loop_.context_window(), Some(1_000_000));
    assert_eq!(loop_.pressure_window(), Some(8_000));
}
