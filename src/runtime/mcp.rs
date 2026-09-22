use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn switch_mcp(&mut self, on: bool) {
        if self.catalog_frozen {
            return;
        }
        if !on {
            self.mcp = None;
            self.mcp_start = None;
            return;
        }
        if self.depth > 0 || self.mcp.is_some() || self.mcp_start.is_some() {
            return;
        }
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            self.mcp_start = Some(runtime.spawn(crate::mcp::Hub::connect(self.workspace.clone())));
        }
    }

    pub(super) async fn settle_mcp(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let Some(start) = self.mcp_start.take() else {
            return Ok(());
        };
        let hub = match start.await {
            Ok(hub) => hub,
            Err(error) => {
                return self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("MCP servers did not start: {error}"),
                    },
                );
            }
        };
        if self.catalog_frozen {
            return Ok(());
        }
        for text in hub.notes.clone() {
            self.push_event(events, emit, Event::Note { text })?;
        }
        self.mcp = Some(Arc::new(hub));
        self.schema = None;
        Ok(())
    }
}
