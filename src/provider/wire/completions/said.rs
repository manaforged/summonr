use serde::Deserialize;

use crate::error::{Error, ProviderFailureKind, Result};
use crate::provider::StopReason;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Finish {
    Stop,
    ToolCalls,
    Length,
    ModelContextWindowExceeded,
    ContentFilter,
    Sensitive,
    NetworkError,
    InsufficientSystemResource,
    #[serde(other)]
    Other,
}

impl Finish {
    pub(super) fn stop(self) -> Result<StopReason> {
        match self {
            Self::Stop => Ok(StopReason::Stop),
            Self::ToolCalls => Ok(StopReason::ToolCalls),
            Self::Length | Self::ModelContextWindowExceeded => Ok(StopReason::Length),
            Self::NetworkError | Self::InsufficientSystemResource => Err(Error::provider(
                ProviderFailureKind::Server,
                format!("the provider cut the reply short: {self:?}"),
            )),
            Self::ContentFilter => Err(crate::provider::wire::blocked("content_filter")),
            Self::Sensitive => Err(crate::provider::wire::blocked("sensitive")),
            Self::Other => Ok(StopReason::Other),
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Said {
    Text(String),
    Chunks(Vec<Chunk>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum Chunk {
    Text {
        text: String,
    },
    Thinking {
        thinking: Vec<Chunk>,
    },
    #[serde(other)]
    Other,
}

impl Chunk {
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Text { text } => text,
            Self::Thinking { .. } | Self::Other => "",
        }
    }
}
