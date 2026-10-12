use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

mod agent;
mod capture;
pub(crate) mod stream;
pub(crate) mod think;
mod wire;
pub(crate) use wire::boundary;

use crate::endpoint::Wire;
pub(crate) use summonr_auth::Auth;

mod client;
mod fit;
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

    fn takes_images(&self) -> bool {
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
pub(crate) use client::{response_failure, unreachable_message};
#[cfg(test)]
pub(crate) use message::Checked;
pub(crate) use message::{
    AssistantMessage, Continuation, Role, Source, StopReason, ToolCall, ToolFunction, ToolKind,
};
pub(crate) use message::{Image, Message};
pub use message::{Usage, UserInput};
pub(crate) use reasoning::{EffortField, Off, Reasoning, Scale, Spec, Step, levels};
pub use request::Effort;
pub(crate) use request::dollars;
pub(crate) use request::{CompletionRequest, FunctionDefinition, ToolDefinition, ToolsJson};
pub(crate) use request::{CountStyle, compact};
pub(crate) use request::{Partial, Price};
#[cfg(test)]
pub(crate) use wire::decisions;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "provider/cache_tests.rs"]
mod cache_tests;

#[cfg(test)]
#[path = "provider/scripted.rs"]
pub(crate) mod scripted;
#[cfg(test)]
#[path = "provider/stall_tests.rs"]
mod stall_tests;

#[cfg(test)]
#[path = "provider/image_tests.rs"]
mod image_tests;
