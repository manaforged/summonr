use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn page_history(&self, direction: f32) {
        let page = (self.content_height.peek() - self.max_scroll.peek()).max(cell().h);
        self.scroll_history(-direction * page);
    }

    pub(in crate::tui::sigil_view) fn history_edge(&self, end: bool) {
        self.scroll_y
            .set(if end { self.max_scroll.peek() } else { 0.0 });
    }

    pub(in crate::tui::sigil_view) fn jump(&self) {
        let rows = match self.search.rows(&self.model.get()) {
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
                if self.model.get().busy_since.is_some() {
                    self.push_entry(Entry::Error(
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
                self.push_entry(Entry::System(
                    "Inserted prompt text. Saved images were not added.".to_string(),
                ));
                self.refresh();
            }
            return;
        }
        if matches!(
            self.model.get().entries.get(*entry),
            Some(Entry::Tool { .. })
        ) && self.model.get().prefs.tools == ToolOutput::Compact
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
        let next = self.scroll_y.peek() - delta;
        self.scroll_y.set(next.clamp(0.0, self.max_scroll.peek()));
    }

    pub(in crate::tui::sigil_view) fn prompt_history(&self, query: &str) {
        self.search.history(query);
        drop(self.cmd.send(Cmd::ListPrompts(None)));
    }
}
