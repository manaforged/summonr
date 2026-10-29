use crate::provider::{AssistantMessage, Continuation, Partial, StopReason, ToolCall, Usage};

#[derive(Default)]
pub(crate) struct ReplyBuilder {
    pub(crate) text: String,
    pub(crate) reasoning: String,
    pub(crate) calls: Vec<ToolCall>,
    pub(crate) usage: Usage,
    pub(crate) stop: StopReason,
    pub(crate) finished: bool,
}

impl ReplyBuilder {
    pub(crate) fn write_partial(&self, partial: &mut Partial) {
        partial.text.push_str(&self.text);
        partial.reasoning.push_str(&self.reasoning);
        if self.usage != Usage::default() {
            partial.usage = Some(self.usage.clone());
        }
    }

    pub(crate) fn finish(self, continuation: Option<Continuation>) -> AssistantMessage {
        AssistantMessage {
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: self.calls,
            reasoning_content: (!self.reasoning.is_empty()).then_some(self.reasoning),
            usage: self.usage,
            stop_reason: self.stop,
            continuation,
        }
    }
}
