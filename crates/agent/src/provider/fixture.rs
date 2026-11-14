use serde_json::Value;

use super::{Client, CompletionRequest, Effort, Message};
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
    let request = CompletionRequest::new(
        "s".to_string(),
        endpoint.model.clone(),
        messages,
        &[],
        max,
        false,
        effort,
    );
    Client::connect(endpoint)
        .expect("the endpoint fixture creates a client")
        .request_body(&request, false)
        .expect("the endpoint request serializes")
}
