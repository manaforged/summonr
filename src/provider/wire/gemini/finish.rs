use serde::Deserialize;

use crate::error::{Error, ProviderFailureKind, Result};
use crate::provider::StopReason;

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Finish {
    Stop,
    MaxTokens,
    Safety,
    Recitation,
    Language,
    Blocklist,
    ProhibitedContent,
    Spii,
    ImageSafety,
    ImageProhibitedContent,
    ImageRecitation,
    Escalation,
    PupLimitedDisabled,
    MalformedFunctionCall,
    UnexpectedToolCall,
    TooManyToolCalls,
    MissingThoughtSignature,
    MalformedResponse,
    #[serde(other)]
    Other,
}

impl Finish {
    pub(super) fn parse(raw: &str) -> Self {
        Self::deserialize(serde::de::value::StrDeserializer::<serde::de::value::Error>::new(raw))
            .unwrap_or(Self::Other)
    }

    pub(super) fn ended(self, raw: &str) -> Result<()> {
        match self {
            Self::Safety
            | Self::Recitation
            | Self::Language
            | Self::Blocklist
            | Self::ProhibitedContent
            | Self::Spii
            | Self::ImageSafety
            | Self::ImageProhibitedContent
            | Self::ImageRecitation
            | Self::Escalation
            | Self::PupLimitedDisabled => Err(crate::provider::wire::blocked(raw)),
            Self::MalformedFunctionCall
            | Self::UnexpectedToolCall
            | Self::TooManyToolCalls
            | Self::MissingThoughtSignature
            | Self::MalformedResponse => Err(Error::provider(
                if matches!(self, Self::MalformedFunctionCall) {
                    ProviderFailureKind::MalformedToolCall
                } else {
                    ProviderFailureKind::Http
                },
                format!("native response ended with {raw}"),
            )),
            Self::Stop | Self::MaxTokens | Self::Other => Ok(()),
        }
    }

    pub(super) fn stop(self) -> StopReason {
        match self {
            Self::Stop => StopReason::Stop,
            Self::MaxTokens => StopReason::Length,
            _ => StopReason::Other,
        }
    }
}
