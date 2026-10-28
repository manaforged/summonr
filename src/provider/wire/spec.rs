use std::future::Future;
use std::pin::Pin;

use summonr_auth::Scheme;

use super::{Chat, Wired, anthropic, completions, gemini, responses};
use crate::provider::{AssistantMessage, Client, CompletionRequest, Partial, Reasoning};
use summonr_core::error::{Error, Result};

pub(crate) struct Spec {
    pub(crate) name: &'static str,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    pub(crate) sentinel: Option<&'static str>,
    pub(crate) probe: Option<&'static str>,
    pub(crate) images: ImageBudget,
    pub(crate) scheme: Scheme,
    pub(crate) transport: Transport,
    pub(crate) encodes_messages: bool,
    pub(crate) plan_session: bool,
}

pub(crate) type Streamer =
    for<'a, 'b> fn(
        &'a Client,
        &'a CompletionRequest<'b>,
        &'a mut Partial,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>>;

pub(crate) type Encoder =
    fn(&CompletionRequest<'_>, &Reasoning, crate::endpoint::CacheTtl, bool) -> Result<Wired>;

#[derive(Clone, Copy)]
pub(crate) struct Codec {
    pub(crate) body: Encoder,
    pub(crate) stream: Streamer,
}

#[derive(Clone, Copy)]
pub(crate) enum Transport {
    Http(Codec),
    Classify,
    Child,
}

impl Transport {
    pub(crate) fn codec(self) -> Result<Codec> {
        match self {
            Self::Http(codec) => Ok(codec),
            Self::Classify => Err(Error::Config(
                "a classify endpoint does not run sessions".to_string(),
            )),
            Self::Child => Err(Error::Config(
                "a Claude Code client sends no HTTP request".to_string(),
            )),
        }
    }
}

impl Spec {
    pub(crate) fn classifies(&self) -> bool {
        matches!(self.transport, Transport::Classify)
    }

    pub(crate) fn child(&self) -> bool {
        matches!(self.transport, Transport::Child)
    }
}

fn codec<W: Chat + 'static>() -> Codec {
    Codec {
        body: W::body,
        stream: crate::provider::stream::streamed::<W>,
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ImageBudget {
    pub(crate) types: &'static [&'static str],
    pub(crate) tool_types: &'static [&'static str],
    pub(crate) count: usize,
    pub(crate) each: usize,
    pub(crate) bytes: usize,
    pub(crate) tool_images: bool,
}

const PNG_JPEG: &[&str] = &["image/png", "image/jpeg"];
const WEB_IMAGES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
const GEMINI_IMAGES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heic",
    "image/heif",
];
const GEMINI_TOOL_IMAGES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

const fn images(
    types: &'static [&'static str],
    count: usize,
    each: usize,
    bytes: usize,
) -> ImageBudget {
    ImageBudget {
        types,
        tool_types: types,
        count,
        each,
        bytes,
        tool_images: true,
    }
}

pub(crate) fn spec(wire: crate::endpoint::Wire) -> Spec {
    let base = Spec {
        name: "completions",
        headers: &[],
        sentinel: None,
        probe: None,
        images: images(PNG_JPEG, 100, 10_000_000, 20_000_000),
        scheme: Scheme::Bearer,
        transport: Transport::Http(codec::<completions::Wire>()),
        encodes_messages: false,
        plan_session: false,
    };
    let anthropic = images(WEB_IMAGES, 100, 10_000_000, 24_000_000);
    match wire {
        crate::endpoint::Wire::Anthropic => Spec {
            name: "anthropic",
            headers: &[("anthropic-version", "2023-06-01")],
            probe: Some("max_input_tokens"),
            images: anthropic,
            scheme: Scheme::ApiKey,
            transport: Transport::Http(codec::<anthropic::Wire>()),
            ..base
        },
        crate::endpoint::Wire::AgentSdk => Spec {
            name: "agent-sdk",
            images: anthropic,
            transport: Transport::Child,
            ..base
        },
        crate::endpoint::Wire::Gemini => Spec {
            name: "gemini",
            probe: Some("inputTokenLimit"),
            images: ImageBudget {
                tool_types: GEMINI_TOOL_IMAGES,
                tool_images: false,
                ..images(GEMINI_IMAGES, 3600, 15_000_000, 15_000_000)
            },
            scheme: Scheme::GoogleKey,
            transport: Transport::Http(codec::<gemini::Wire>()),
            ..base
        },
        crate::endpoint::Wire::Responses => Spec {
            name: "responses",
            images: images(WEB_IMAGES, 1500, 256_000_000, 256_000_000),
            transport: Transport::Http(codec::<responses::Wire>()),
            plan_session: true,
            ..base
        },
        crate::endpoint::Wire::Completions => Spec {
            sentinel: Some("[DONE]"),
            encodes_messages: true,
            ..base
        },
        crate::endpoint::Wire::Decisions => Spec {
            name: "decisions",
            sentinel: Some("[DONE]"),
            images: images(PNG_JPEG, 0, 0, 0),
            transport: Transport::Classify,
            ..base
        },
    }
}
