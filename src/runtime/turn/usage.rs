use super::*;

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) fn record_usage(
        &mut self,
        assistant: &AssistantMessage,
        prefix_hash: String,
        role: workflow::Role,
        usage: &mut Usage,
    ) -> UsageMeta {
        self.count_usage(assistant, usage);
        UsageMeta {
            continuation: assistant.continuation.clone(),
            prefix_hash,
            cost: self.price_for(role).map(|price| {
                price.cost(
                    u64::from(assistant.usage.prompt_tokens),
                    u64::from(assistant.usage.completion_tokens),
                    u64::from(assistant.usage.cached_tokens),
                    u64::from(assistant.usage.cache_write_tokens),
                )
            }),
        }
    }

    pub(in crate::runtime) fn count_usage(
        &mut self,
        assistant: &AssistantMessage,
        usage: &mut Usage,
    ) {
        usage.add(&assistant.usage);
        self.spent_tokens +=
            u64::from(assistant.usage.prompt_tokens + assistant.usage.completion_tokens);
    }

    pub(in crate::runtime) fn emit_records(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        for line in self.session.take_pending() {
            for event in line.into_events(crate::durable::projection::Speech::Skip) {
                self.push_event(events, emit, event)?;
            }
        }
        Ok(())
    }
}
