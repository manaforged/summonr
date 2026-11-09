use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;

use serde::Deserialize;
use summonr_auth::Scheme;

use super::{Chat, Encoding, Wired, anthropic, completions, gemini, responses};
use crate::endpoint::Wire;
use crate::provider::{AssistantMessage, Client, CompletionRequest, Partial};
use summonr_core::error::Result;

#[derive(Deserialize)]
pub(crate) struct WireSpec {
    #[serde(deserialize_with = "crate::endpoint::deserialize_wire")]
    pub(crate) wire: Wire,
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) path: String,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) sentinel: Option<String>,
    #[serde(default)]
    pub(crate) probe: Option<String>,
    pub(crate) images: ImageBudget,
    pub(crate) scheme: Scheme,
    #[serde(default)]
    pub(crate) subscription_session: bool,
}

#[derive(Clone, Deserialize)]
pub(crate) struct ImageBudget {
    pub(crate) types: Vec<String>,
    pub(crate) tool_types: Vec<String>,
    pub(crate) count: usize,
    pub(crate) each: usize,
    pub(crate) bytes: usize,
    pub(crate) tool_images: bool,
}

pub(crate) type Streamer =
    for<'a, 'b> fn(
        &'a Client,
        &'a CompletionRequest<'b>,
        &'a mut Partial,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>>;

pub(crate) type Encoder = fn(&CompletionRequest<'_>, &Encoding<'_>) -> Result<Wired>;

#[derive(Clone, Copy)]
pub(crate) struct Codec {
    pub(crate) body: Encoder,
    pub(crate) stream: Streamer,
}

fn of<W: Chat + 'static>() -> Codec {
    Codec {
        body: W::body,
        stream: crate::provider::stream::streamed::<W>,
    }
}

pub(crate) fn codec(wire: Wire) -> Option<Codec> {
    match wire {
        Wire::Completions => Some(of::<completions::Wire>()),
        Wire::Responses => Some(of::<responses::Wire>()),
        Wire::Anthropic => Some(of::<anthropic::Wire>()),
        Wire::Gemini => Some(of::<gemini::Wire>()),
        Wire::Decisions | Wire::AgentSdk => None,
    }
}

impl WireSpec {
    pub(crate) fn classifies(&self) -> bool {
        self.wire == Wire::Decisions
    }

    pub(crate) fn child(&self) -> bool {
        self.wire == Wire::AgentSdk
    }

    pub(crate) fn url(&self, base: &str, model: &str) -> String {
        format!("{base}{}", self.path.replace("{model}", model))
    }
}

#[derive(Deserialize)]
struct Wires {
    wire: Vec<WireSpec>,
}

static WIRES: LazyLock<Vec<WireSpec>> = LazyLock::new(|| {
    let table: Wires = summonr_core::embedded!("wire", "provider/wire/wires.toml");
    table.wire
});

pub(crate) fn spec(wire: Wire) -> &'static WireSpec {
    match WIRES.iter().find(|row| row.wire == wire) {
        Some(row) => row,
        None => panic!("the built-in wire table has no row for {wire:?}"),
    }
}
