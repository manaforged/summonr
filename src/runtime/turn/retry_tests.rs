use super::{retry_cause, retryable_failure};
use crate::error::{ProviderFailure, ProviderFailureKind};

#[test]
fn an_authentication_failure_is_not_retried() {
    let failure = ProviderFailure {
        kind: ProviderFailureKind::Authentication,
        status: Some(401),
        message: "rejected the key".to_string(),
        partial_output: true,
        retry_after: None,
    };
    assert!(!retryable_failure(&failure));
}

fn limited(retry_after: Option<u64>) -> ProviderFailure {
    ProviderFailure {
        kind: ProviderFailureKind::RateLimited,
        status: Some(429),
        message: "slow down".to_string(),
        partial_output: false,
        retry_after,
    }
}

#[test]
fn a_long_retry_after_falls_back_instead_of_retrying() {
    assert!(retryable_failure(&limited(None)));
    assert!(retryable_failure(&limited(Some(30))));
    assert!(!retryable_failure(&limited(Some(120))));
}

#[test]
fn a_connect_failure_is_named_as_one() {
    let failure = ProviderFailure {
        kind: ProviderFailureKind::Transport,
        status: None,
        message: "connection refused".to_string(),
        partial_output: false,
        retry_after: None,
    };
    assert!(retryable_failure(&failure));
    assert_eq!(retry_cause(&failure), "connection failed");
    assert_eq!(retry_cause(&limited(None)), "rate limited");
}
