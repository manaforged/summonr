use super::*;

impl<P: Provider> Loop<P> {
    pub(in crate::runtime) fn emit_records(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if let Some(text) = self.session.take_notice() {
            self.push_event(events, emit, Event::Note { text })?;
        }
        for line in self.session.take_pending() {
            for event in line.into_events(crate::durable::projection::AssistantEvents::Skip) {
                self.push_event(events, emit, event)?;
            }
        }
        Ok(())
    }
}
