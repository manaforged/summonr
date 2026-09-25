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

    pub(in crate::tui::sigil_view) fn queue(&self) {
        if matches!(self.model.get().busy_since, Some((Activity::Branch, _))) {
            return;
        }
        self.splash.set(false);
        let text = self.expand_pastes(&self.draft.peek());
        if text.trim().is_empty() && self.attachments.borrow().is_empty() {
            return;
        }
        let Some(mut input) = self.attach(&text) else {
            return;
        };
        let pending = self.take_attachments();
        self.replace(String::new());
        self.pasted_text.borrow_mut().clear();
        self.model_mut(|model| {
            model.entries.push(Entry::User(text.clone()));
            for attachment in pending {
                if let Some(source) = attachment.source {
                    model.entries.push(Entry::Image {
                        source,
                        bytes: attachment.image.bytes().unwrap_or_default().into(),
                        label: attachment.label,
                    });
                }
                input.images.push(attachment.image);
            }
            model
                .entries
                .push(Entry::System("queued for the next turn".to_string()));
        });
        drop(self.cmd.send(Cmd::Queue(input)));
        self.scroll_y.set(self.max_scroll.peek());
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn queue_if_busy(&self) -> bool {
        if self.model.get().busy_since.is_none() || self.draft.peek().trim().is_empty() {
            return false;
        }
        self.queue();
        true
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
        self.reveal(draft);
        if draft.contains('@') && !self.commands.mentions() {
            drop(self.cmd.send(Cmd::ListFiles));
        }
        self.commands.change(
            draft,
            &self.model.peek().snapshot.endpoint,
            &self.picker.facts.peek(),
        );
    }

    pub(in crate::tui::sigil_view) fn reveal(&self, draft: &str) {
        if self.splash.peek() {
            let (cols, rows) = self.viewport.peek();
            let width = super::scene::prompt_area(self, cols, rows)[2].saturating_sub(9);
            if draft.contains('\n')
                || draft.trim_start().starts_with('/')
                || draft.contains('@')
                || cell_width(draft) > usize::from(width)
            {
                self.splash.set(false);
            }
        }
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
        self.reveal(draft);
        self.caret.set(draft.len());
        self.draft.set(draft.to_string());
    }

    pub(in crate::tui::sigil_view) fn chat_cols(&self) -> u16 {
        self.layout.get().split(self.viewport.get().0).0
    }
}
