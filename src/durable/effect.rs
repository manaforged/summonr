use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::bound::{Items, Text};
use super::id::{
    ContextEntryId, ContextEpochId, Digest, OperationId, ProviderAttemptId, ProviderResultId,
    ToolAttemptId, ToolBatchId, ToolCallId, ToolResultId,
};

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_DIAGNOSTIC_BYTES: usize = 65_536;
const MAX_TOOL_CALLS: usize = 128;
const MAX_TOOL_ARGUMENT_BYTES: usize = 1_048_576;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum EffectError {
    #[error("provider request cannot be empty")]
    EmptyRequest,
    #[error("tool call positions must be strictly increasing")]
    ToolOrder,
    #[error("tool call or result identity is duplicated in one batch")]
    ToolIdentity,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderPurpose {
    AgentRound,
    CompactionSummary,
    GoalJudge,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Exposure {
    pub(crate) max_input_tokens: u64,
    pub(crate) max_output_tokens: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderIntent {
    pub(crate) attempt: ProviderAttemptId,
    pub(crate) result: ProviderResultId,
    pub(crate) operation: OperationId,
    pub(crate) epoch: ContextEpochId,
    pub(crate) purpose: ProviderPurpose,
    pub(crate) through: Option<ContextEntryId>,
    pub(crate) request_bytes: u32,
    pub(crate) request_digest: Digest,
    pub(crate) exposure: Exposure,
}

impl ProviderIntent {
    pub(crate) fn validate(&self) -> Result<(), EffectError> {
        if self.request_bytes == 0 {
            return Err(EffectError::EmptyRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Usage {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cached_input_tokens: Option<u64>,
    pub(crate) live_millis: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderToolCall {
    pub(crate) provider_id: Text<1_024>,
    pub(crate) name: Text<1_024>,
    pub(crate) arguments: Text<MAX_TOOL_ARGUMENT_BYTES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderMessage {
    pub(crate) text: Text<MAX_TEXT_BYTES>,
    pub(crate) reasoning: Text<MAX_TEXT_BYTES>,
    pub(crate) tool_calls: Items<ProviderToolCall, MAX_TOOL_CALLS>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub(crate) enum ProviderOutcome {
    Completed {
        message: ProviderMessage,
        reported_usage: Usage,
    },
    Failed {
        kind: ProviderFailureKind,
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
        partial_usage: Usage,
    },
    Interrupted {
        reason: Text<MAX_DIAGNOSTIC_BYTES>,
        partial_usage: Usage,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderFailureKind {
    Transport,
    RateLimit,
    Authentication,
    ContextOverflow,
    Other,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ToolCall {
    pub(crate) id: ToolCallId,
    pub(crate) batch: ToolBatchId,
    pub(crate) result: ToolResultId,
    pub(crate) provider_result: ProviderResultId,
    pub(crate) provider_id: Text<1_024>,
    pub(crate) position: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ToolIntent {
    pub(crate) attempt: ToolAttemptId,
    pub(crate) call: ToolCallId,
    pub(crate) normalized_arguments: Text<MAX_TOOL_ARGUMENT_BYTES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub(crate) enum ToolOutcome {
    Completed {
        output: Text<MAX_TEXT_BYTES>,
    },
    Failed {
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
    Cancelled {
        state: ToolCancellation,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolCancellation {
    Prestart,
    Stopped,
}

impl ToolOutcome {
    pub(crate) fn needs_start(&self) -> bool {
        !matches!(
            self,
            Self::Cancelled {
                state: ToolCancellation::Prestart
            }
        )
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ToolResult {
    pub(crate) id: ToolResultId,
    pub(crate) call: ToolCallId,
    pub(crate) attempt: Option<ToolAttemptId>,
    pub(crate) outcome: ToolOutcome,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "settlement", content = "data", rename_all = "snake_case")]
pub(crate) enum MeterSettlement {
    Known(Usage),
    Estimated(Usage),
}

#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BudgetKind {
    SessionTokens,
    SessionLiveMillis,
}

pub(crate) fn validate_calls(calls: &[&ToolCall]) -> Result<(), EffectError> {
    if calls
        .windows(2)
        .any(|pair| pair[0].position >= pair[1].position)
    {
        return Err(EffectError::ToolOrder);
    }
    for (index, call) in calls.iter().enumerate() {
        if calls[..index]
            .iter()
            .any(|prior| prior.provider_id == call.provider_id || prior.result == call.result)
        {
            return Err(EffectError::ToolIdentity);
        }
    }
    Ok(())
}
