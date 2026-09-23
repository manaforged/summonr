use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn submit(&self) {
        if matches!(self.model.borrow().busy_since, Some((Activity::Branch, _))) {
            return;
        }
        if self.view_edit.peek() {
            return;
        }
        let text = self.expand_pastes(&self.draft.peek());
        if text.trim().is_empty() && self.attachments.borrow().is_empty() {
            return;
        }
        let argument = match argument(&text) {
            Ok(argument) => argument,
            Err(error) => {
                self.model.borrow_mut().entries.push(Entry::Error(error));
                self.splash.set(false);
                self.refresh();
                return;
            }
        };
        if self.local(&text, argument) {
            self.replace(String::new());
            return;
        }
        if text.trim_start().starts_with('/') {
            self.replace(String::new());
            self.scroll_y.set(self.extent.peek());
            drop(self.cmd.send(Cmd::Command(text)));
            return;
        }
        let peers = super::suggest::peers(&text, &self.model.borrow());
        if !peers.is_empty() {
            self.replace(String::new());
            self.splash.set(false);
            self.model
                .borrow_mut()
                .entries
                .push(Entry::User(text.clone()));
            drop(self.cmd.send(Cmd::Send(peers, text)));
            self.refresh();
            return;
        }
        let Some(mut input) = self.attach(&text) else {
            return;
        };
        let pending = self.take_attachments();
        self.replace(String::new());
        self.pasted_text.borrow_mut().clear();
        if input.text.trim().is_empty() && input.images.is_empty() && pending.is_empty() {
            return;
        }
        self.splash.set(false);
        {
            let mut model = self.model.borrow_mut();
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
        }
        let activity = self.model.borrow().busy_since.map(|(activity, _)| activity);
        let command = match activity {
            Some(Activity::Turn) => Cmd::Steer(input),
            Some(_) => {
                self.model
                    .borrow_mut()
                    .entries
                    .push(Entry::System("queued for the next turn".to_string()));
                Cmd::Queue(input)
            }
            None => Cmd::Submit(input),
        };
        drop(self.cmd.send(command));
        self.scroll_y.set(self.extent.peek());
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn retry(&self) {
        if self.model.borrow().busy_since.is_some() {
            self.model.borrow_mut().entries.push(Entry::System(
                "Cancel the active turn before retrying.".to_string(),
            ));
            self.refresh();
            return;
        }
        drop(self.cmd.send(Cmd::Command("/retry".to_string())));
    }

    pub(in crate::tui::sigil_view) fn complete(&self) -> bool {
        if self.settings_open.peek() || self.model.borrow().busy_since.is_some() {
            return false;
        }
        let typed = self.draft.peek();
        let Some(name) = self
            .commands
            .complete(&typed, self.caret.peek(), &self.model.borrow())
        else {
            return false;
        };
        self.completion(&name);
        true
    }

    pub(in crate::tui::sigil_view) fn accept(&self) -> bool {
        if self.settings_open.peek() || self.model.borrow().busy_since.is_some() {
            return false;
        }
        let typed = self.draft.peek();
        let Some(name) = self
            .commands
            .accept(&typed, self.caret.peek(), &self.model.borrow())
        else {
            return false;
        };
        self.completion(&name);
        true
    }

    pub(in crate::tui::sigil_view) fn escape_idle(&self) -> bool {
        if !armed(&self.escape_armed, REPEAT_WINDOW) {
            return false;
        }
        self.local("/tree", None)
    }

    pub(in crate::tui::sigil_view) fn dismiss(&self) -> bool {
        self.commands
            .hide(&self.draft.peek(), self.caret.peek(), &self.model.borrow())
    }

    pub(in crate::tui::sigil_view) fn interrupt(&self) {
        if self.overlay_close() {
            return;
        }
        if let Some((activity, _)) = self.model.borrow().busy_since {
            if activity != Activity::Branch {
                drop(self.cmd.send(Cmd::Cancel));
            }
        } else if self.draft.peek().is_empty() && self.attachments.borrow().is_empty() {
            if armed(&self.quit_armed, REPEAT_WINDOW) {
                self.quit.set(true);
            } else {
                self.model
                    .borrow_mut()
                    .entries
                    .push(Entry::System("press ctrl+c again to quit".to_string()));
                self.refresh();
            }
        } else {
            self.replace(String::new());
            self.pasted_text.borrow_mut().clear();
            if !self.take_attachments().is_empty() {
                self.model.borrow_mut().entries.push(Entry::System(
                    "Draft discarded. Attached images were removed.".to_string(),
                ));
                self.refresh();
            }
        }
    }
}
