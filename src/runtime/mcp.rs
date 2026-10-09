use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn switch_mcp(&mut self, on: bool) {
        if self.catalog_frozen {
            return;
        }
        if !on {
            self.mcp = None;
            self.mcp_wanted = false;
            if let Ok(mut board) = self.mcp_board.lock() {
                *board = std::sync::Weak::new();
            }
            return;
        }
        self.mcp_wanted = self.depth == 0 && self.mcp.is_none();
    }

    pub(super) async fn settle_mcp(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if !std::mem::take(&mut self.mcp_wanted) || self.catalog_frozen {
            return Ok(());
        }
        let hub = crate::mcp::Hub::connect(self.workspace.clone(), self.tools.sandbox()).await;
        for text in hub.notes.clone() {
            self.push_event(events, emit, Event::Note { text })?;
        }
        let hub = Arc::new(hub);
        if let Ok(mut board) = self.mcp_board.lock() {
            *board = Arc::downgrade(&hub);
        }
        self.mcp = Some(hub);
        self.schema = None;
        Ok(())
    }

    pub(super) async fn expand_mcp(
        &mut self,
        input: UserInput,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<UserInput> {
        if !crate::mcp::wanted(&input.text) {
            return Ok(self.expand_command(input));
        }
        self.settle_mcp(events, emit).await?;
        let Some(hub) = self.mcp.clone() else {
            return Ok(self.expand_command(input));
        };
        let head = input.text.split_whitespace().next().unwrap_or_default();
        let offered = crate::mcp::offers(&self.workspace)
            .iter()
            .any(|offer| offer.kind == crate::mcp::OfferKind::Prompt && offer.insert == head);
        let input = if offered {
            input
        } else {
            self.expand_command(input)
        };
        hub.expand(input, self.tools.sandbox().as_ref()).await
    }

    fn expand_command(&self, input: UserInput) -> UserInput {
        let Some(text) = crate::plugins::commands::expand(&input.text, &self.workspace) else {
            return input;
        };
        let source = input.source.unwrap_or(crate::provider::Source {
            text: input.text,
            images: input.images.len(),
        });
        UserInput {
            text,
            images: input.images,
            source: Some(source),
        }
    }

    #[must_use]
    pub fn mcp_board(&self) -> crate::mcp::Board {
        Arc::clone(&self.mcp_board)
    }
}
