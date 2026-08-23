//! Project the durable session state back into provider conversation history.
//!
//! The durable journal is the source of truth; the provider needs an ordered
//! [`Message`] stream. This module rebuilds that stream from the reduced
//! [`SessionState`]: the active epoch's ordered context entries, with
//! assistant message content, reasoning, and tool calls recovered from the
//! recorded provider outcome, and tool results recovered from the tool state
//! that produced each tool context entry.

use crate::provider::{Image, Message, Role, ToolCall, ToolFunction, ToolKind};
use crate::session::Replay;

use super::context::{ContextEntry, ContextRole, ContextSource};
use super::effect::ProviderOutcome;
use super::reducer::{ProviderState, SessionState};

/// Rebuild the provider conversation from a reduced durable state.
pub(crate) fn replay(state: &SessionState) -> Replay {
    let mut history = Vec::new();
    if let Some(epoch) = state.current_epoch
        && let Some(entry_ids) = state.epoch_entries.get(&epoch)
    {
        for id in entry_ids {
            if let Some(entry) = state.entries.get(id) {
                history.push(project_entry(state, entry));
            }
        }
    }
    Replay {
        history,
        task: None,
        prompt: state
            .current_epoch
            .and_then(|id| state.epochs.get(&id))
            .map(|epoch| epoch.head.system.as_str().to_string()),
        model: state
            .current_epoch
            .and_then(|id| state.epochs.get(&id))
            .map(|epoch| epoch.head.model.as_str().to_string()),
        id: state.session.map(|id| id.to_string()).unwrap_or_default(),
        spent_tokens: 0,
    }
}

/// Project one context entry into a provider [`Message`].
pub(crate) fn project_entry(state: &SessionState, entry: &ContextEntry) -> Message {
    match entry.role {
        ContextRole::System => Message::system(entry.payload.as_str()),
        ContextRole::User => project_user(state, entry),
        ContextRole::Assistant => project_assistant(state, entry),
        ContextRole::Tool => project_tool(state, entry),
    }
}

fn project_user(state: &SessionState, entry: &ContextEntry) -> Message {
    let mut message = Message::user(entry.payload.as_str().to_string());
    let ContextSource::Input(input) = entry.source else {
        return message;
    };
    let Some(payload) = state.input_payloads.get(&input) else {
        return message;
    };
    message.images = payload
        .images
        .as_slice()
        .iter()
        .map(|image| Image {
            media_type: image.media_type.as_str().to_string(),
            bytes: image.data.as_slice().to_vec(),
        })
        .collect();
    message
}

/// Recover an assistant message, including reasoning and tool calls.
fn project_assistant(state: &SessionState, entry: &ContextEntry) -> Message {
    let mut message = Message {
        role: Role::Assistant,
        content: entry.payload.as_str().to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning: None,
    };
    let ContextSource::Provider(result_id) = &entry.source else {
        return message;
    };
    let Some(ProviderOutcome::Completed {
        message: provider_message,
        ..
    }) = provider_outcome(state, *result_id)
    else {
        return message;
    };
    if provider_message.reasoning.len() > 0 {
        message.reasoning = Some(provider_message.reasoning.as_str().to_string());
    }
    message.tool_calls = provider_message
        .tool_calls
        .as_slice()
        .iter()
        .map(|call| ToolCall {
            id: call.provider_id.as_str().to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: call.name.as_str().to_string(),
                arguments: call.arguments.as_str().to_string(),
            },
        })
        .collect();
    message
}

/// Recover a tool result message, linking it to the issued call id.
fn project_tool(state: &SessionState, entry: &ContextEntry) -> Message {
    Message::tool(tool_call_id_for(state, entry), entry.payload.as_str())
}

/// Find the provider-side id of the tool call that produced a tool entry.
fn tool_call_id_for(state: &SessionState, entry: &ContextEntry) -> String {
    let ContextSource::Tool(result_id) = &entry.source else {
        return String::new();
    };
    state
        .tools
        .values()
        .find(|tool| tool.call.result == *result_id)
        .map(|tool| tool.call.provider_id.as_str().to_string())
        .unwrap_or_default()
}

fn provider_outcome(
    state: &SessionState,
    result_id: super::id::ProviderResultId,
) -> Option<&ProviderOutcome> {
    state.providers.iter().find_map(|(_, provider)| {
        let ProviderState::Finished { intent, outcome } = provider else {
            return None;
        };
        (intent.result == result_id).then_some(outcome)
    })
}
