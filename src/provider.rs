use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use summonr_core::error::{Error, ProviderFailure, ProviderFailureKind, Result};

mod agent;
mod capture;
pub(crate) mod stream;
pub(crate) mod think;
pub(crate) mod wire;
pub(crate) use wire::boundary;
pub(crate) use wire::traits::traits;

use crate::endpoint::Wire;
pub(crate) use summonr_auth::Auth;

mod client;
mod fit;
pub(crate) use fit::fit;
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

    fn takes_images(&self) -> bool {
        false
    }

    fn supports_children(&self) -> bool {
        false
    }
}

type DeltaFn = Arc<dyn Fn(&wire::Delta) + Send + Sync>;

pub use client::Client;
pub(crate) use client::input_cap;
#[cfg(test)]
pub(crate) use client::{response_failure, unreachable_message};
pub(crate) use reasoning::{EffortField, Off, Reasoning, Scale, Spec, Step, levels};
pub(crate) use request::dollars;
pub(crate) use request::{CompletionRequest, ToolsJson};
pub(crate) use request::{CountStyle, compact};
pub use summonr_types::Effort;
pub(crate) use summonr_types::Partial;
pub(crate) use summonr_types::Price;
pub(crate) use summonr_types::ToolDefinition;
pub(crate) use summonr_types::{
    AssistantMessage, Continuation, Role, Source, StopReason, ToolCall, ToolFunction, ToolKind,
};
#[cfg(test)]
pub(crate) use summonr_types::{Checked, FunctionDefinition};
pub(crate) use summonr_types::{Image, Message};
pub use summonr_types::{Usage, UserInput};
pub use wire::Delta;
#[cfg(test)]
pub(crate) use wire::decisions;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider/cache_tests.rs"]
mod cache_tests;
#[cfg(test)]
#[path = "provider/host_cache_tests.rs"]
mod host_cache_tests;

#[cfg(test)]
#[path = "provider/scripted.rs"]
pub(crate) mod scripted;
#[cfg(test)]
#[path = "provider/stall_tests.rs"]
mod stall_tests;

#[cfg(test)]
#[path = "provider/image_tests.rs"]
mod image_tests;
