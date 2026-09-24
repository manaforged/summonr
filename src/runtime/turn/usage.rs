use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn seat_usage(
        &mut self,
        role: workflow::Role,
        before: &Usage,
        after: &Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.sync_usage(emit);
        let prompt = after.prompt_tokens.saturating_sub(before.prompt_tokens);
        let completion = after
            .completion_tokens
            .saturating_sub(before.completion_tokens);
        if prompt == 0 && completion == 0 {
            return Ok(());
        }
        let cached = after.cached_tokens.saturating_sub(before.cached_tokens);
        self.push_event(
            events,
            emit,
            Event::Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                reasoning_tokens: after
                    .reasoning_tokens
                    .saturating_sub(before.reasoning_tokens),
                cached_tokens: cached,
                millis: None,
                prefix_hash: format!("{role:?}").to_lowercase(),
                cost: self.price_for(role).map(|price| {
                    price.cost(u64::from(prompt), u64::from(completion), u64::from(cached))
                }),
            },
        )
    }

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
            for event in line.events() {
                if matches!(event, Event::Reasoning { .. } | Event::Assistant { .. }) {
                    continue;
                }
                self.push_event(events, emit, event)?;
            }
        }
        Ok(())
    }
}
