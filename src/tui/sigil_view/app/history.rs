use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn page_history(&self, direction: f32) {
        let page = self.transcript_height.peek().max(cell().h);
        self.scroll_history(-direction * page);
    }

    pub(in crate::tui::sigil_view) fn jump(&self) {
        let rows = match self.search.rows(&self.model.borrow()) {
            Ok(rows) => rows,
            Err(_) => return,
        };
        let Some((entry, _)) = rows.get(
            self.search
                .selected
                .peek()
                .min(rows.len().saturating_sub(1)),
        ) else {
            return;
        };
        if self.search.is_history() {
            let Some(prompt) = self.search.prompt(*entry) else {
                return;
            };
            if self.search.is_branch() {
                if self.model.borrow().busy_since.is_some() {
                    self.model.borrow_mut().entries.push(Entry::Error(
                        "Wait for the active operation or cancel it before branching.".to_string(),
                    ));
                    self.search.open.set(false);
                    self.refresh();
                    return;
                }
                drop(self.cmd.send(Cmd::Branch {
                    session: prompt.session.id.clone(),
                    turn: prompt.turn,
                }));
                self.update(Msg::Busy(Some(Activity::Branch)));
                self.search.open.set(false);
                self.refresh();
                return;
            }
            self.search.open.set(false);
            self.editor.insert(&prompt.text);
            self.change(&self.draft.peek());
            if prompt.images > 0 {
                self.model.borrow_mut().entries.push(Entry::System(
                    "Inserted prompt text. Saved images were not added.".to_string(),
                ));
                self.refresh();
            }
            return;
        }
        if matches!(
            self.model.borrow().entries.get(*entry),
            Some(Entry::Tool { .. })
        ) && self.model.borrow().prefs.tools == ToolOutput::Compact
        {
            self.toggle_tools();
        }
        batch(|| {
            self.search.target.set(Some(*entry));
            self.search.open.set(false);
            self.splash.set(false);
            self.refresh();
        });
    }

    pub(in crate::tui::sigil_view) fn scroll_history(&self, delta: f32) {
        let extent = self.extent.peek();
        let next = self.scroll_y.peek() - delta;
        self.scroll_y.set(next.clamp(0.0, extent));
    }

    pub(in crate::tui::sigil_view) fn reflow(&self) {
        if !self.tui_chat_active.peek() {
            return;
        }
        let pinned = self.scroll_y.peek() >= self.extent.peek();
        let mut history = self.history.borrow_mut();
        let anchor = if pinned {
            None
        } else {
            history.anchor(self.scroll_y.peek())
        };
        let model = self.model.borrow();
        let total = history.total_extent(&model, self.chat_cols(), model.entries.stamp);
        let extent = (total - self.transcript_height.peek()).max(0.0);
        let position = if pinned {
            extent
        } else {
            anchor
                .and_then(|anchor| history.locate(anchor))
                .unwrap_or_else(|| self.scroll_y.peek())
                .min(extent)
        };
        drop(history);
        self.extent.set(extent);
        self.scroll_y.set(position);
    }

    pub(in crate::tui::sigil_view) fn prompt_history(&self, query: &str) {
        self.search.history(query);
        drop(self.cmd.send(Cmd::ListPrompts(None)));
    }
}
