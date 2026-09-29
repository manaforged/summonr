use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

mod capture;
pub(crate) mod stream;
pub(crate) mod think;
mod wire;

use crate::endpoint::Wire;
pub(crate) use summonr_auth::Auth;

mod client;
mod message;
mod reasoning;
mod request;

pub trait Provider: Send + Sync + 'static {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>>;

    fn stream<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
        _partial: &'a mut Partial,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        self.complete(request)
    }

    fn child(&self) -> Option<Self>
    where
        Self: Sized,
    {
        None
    }

    fn keeps_reasoning(&self) -> bool {
        false
    }

    fn supports_children(&self) -> bool {
        false
    }
}

type DeltaFn = Arc<dyn Fn(&str) + Send + Sync>;

pub use client::Client;
pub(crate) use client::input_cap;
#[cfg(test)]
pub(crate) use client::response_failure;
pub use message::*;
pub(crate) use reasoning::{EffortField, Off, Reasoning, Scale, Spec, Step, levels};
pub use request::*;
#[cfg(test)]
pub(crate) use wire::decisions;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider/stall_tests.rs"]
mod stall_tests;
