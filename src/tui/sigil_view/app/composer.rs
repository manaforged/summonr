use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn shift(&self, delta: isize) -> bool {
        self.commands.shift(
            &self.draft.peek(),
            self.caret.peek(),
            &self.model.get(),
            delta,
        )
    }

    pub(in crate::tui::sigil_view) fn dequeue(&self) {
        if self.model.get().queued != (0, 0) {
            drop(self.cmd.send(Cmd::Dequeue));
        }
    }

    pub(in crate::tui::sigil_view) fn replace(&self, draft: String) {
        self.change(&draft);
        self.caret.set(draft.len());
        self.draft.set(draft);
    }

    pub(in crate::tui::sigil_view) fn edit(&self, draft: &str) {
        self.pane_focus.set(false);
        self.editor.replace(draft);
        self.change(&self.draft.peek());
    }

    pub(in crate::tui::sigil_view) fn change(&self, draft: &str) {
        self.recalled.set(None);
        if draft.contains('@') && !self.commands.mentions() {
            drop(self.cmd.send(Cmd::ListFiles));
        }
        self.commands.change(
            draft,
            &self.model.peek().snapshot.levels,
            &self.model.peek().snapshot.endpoint,
            &self.picker.facts.peek(),
        );
    }

    pub(in crate::tui::sigil_view) fn boundary(&self, end: bool, extend: bool) -> bool {
        if self.options_active() || self.view_edit.peek() || self.pane_active().is_some() {
            return false;
        }
        self.editor
            .select(if end { self.draft.peek().len() } else { 0 }, extend);
        true
    }

    pub(in crate::tui::sigil_view) fn recall(&self, older: bool) -> bool {
        if self.options_active() || self.view_edit.peek() {
            return false;
        }
        let draft = self.draft.peek();
        if older
            && draft.is_empty()
            && let Some(text) = self.cleared.peek()
        {
            self.cleared.set(None);
            self.replace(text);
            return true;
        }
        let caret = self.caret.peek().min(draft.len());
        let edge = if older {
            &draft[..caret]
        } else {
            &draft[caret..]
        };
        if edge.contains('\n') {
            return false;
        }
        let model = self.model.get();
        let (index, saved, saved_caret) = self
            .recalled
            .peek()
            .unwrap_or_else(|| (model.entries.len(), draft.clone(), caret));
        let prompts = model
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| match entry {
                Entry::User(text) if !text.is_empty() => Some((index, text)),
                _ => None,
            });
        let picked = if older {
            prompts.rev().find(|(candidate, _)| *candidate < index)
        } else {
            prompts
                .into_iter()
                .find(|(candidate, _)| *candidate > index)
        };
        if let Some((index, text)) = picked {
            self.select(text);
            self.commands.hide(text, text.len(), &model);
            self.recalled.set(Some((index, saved, saved_caret)));
            true
        } else if !older && self.recalled.peek().is_some() {
            self.replace(saved);
            self.caret.set(saved_caret);
            true
        } else {
            false
        }
    }

    pub(in crate::tui::sigil_view) fn select(&self, draft: &str) {
        self.caret.set(draft.len());
        self.draft.set(draft.to_string());
    }

    pub(in crate::tui::sigil_view) fn chat_cols(&self) -> u16 {
        self.layout.get().split(self.viewport.get().0).0
    }
}
