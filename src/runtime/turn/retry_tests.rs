use super::retryable_failure;
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
