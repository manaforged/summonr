use super::*;

const QUEUED_NOW: &str = "queued · ↵ sends it now";
const QUEUED_LATER: &str = "queued for the next turn";

impl App {
    pub(in crate::tui::sigil_view) fn submit(&self) {
        if matches!(self.model.get().busy_since, Some((Activity::Branch, _))) {
            return;
        }
        if self.view_edit.peek() {
            return;
        }
        let text = self.submit_text();
        if text.trim().is_empty() && self.attachments.borrow().is_empty() {
            self.steer_queued();
            return;
        }
        if self.dispatch_line(&text) {
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
        self.record_user(&text, pending, &mut input);
        let command = self.queue_command(input);
        drop(self.cmd.send(command));
        self.scroll_y.set(self.max_scroll.peek());
        self.refresh();
    }

    fn submit_text(&self) -> String {
        let mut text = self.expand_pastes(&self.draft.peek());
        if let Some((line, note)) = crate::command::renamed(&text) {
            if let Some(note) = note {
                self.push_entry(Entry::System(note));
            }
            text = line;
        }
        text
    }

    fn dispatch_line(&self, text: &str) -> bool {
        let argument = match argument(text) {
            Ok(argument) => argument,
            Err(error) => {
                self.push_entry(Entry::Error(error));
                self.splash.set(false);
                self.refresh();
                return true;
            }
        };
        if self.local(text, argument) {
            self.replace(String::new());
            return true;
        }
        if text.trim_start().starts_with('/') && !crate::command::server_prompt(text) {
            self.replace(String::new());
            self.scroll_y.set(self.max_scroll.peek());
            drop(self.cmd.send(Cmd::Command(text.to_owned())));
            return true;
        }
        let peers = super::suggest::peers(text, &self.model.get());
        if !peers.is_empty() {
            self.replace(String::new());
            self.splash.set(false);
            self.push_entry(Entry::User(text.to_owned()));
            drop(self.cmd.send(Cmd::Send(peers, text.to_owned())));
            self.refresh();
            return true;
        }
        false
    }

    fn record_user(&self, text: &str, pending: Vec<Attachment>, input: &mut UserInput) {
        self.model_mut(|model| {
            model.entries.push(Entry::User(text.to_owned()));
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
        });
    }

    fn queue_command(&self, input: UserInput) -> Cmd {
        let activity = self.model.get().busy_since.map(|(activity, _)| activity);
        match activity {
            Some(Activity::Turn) => {
                self.push_entry(Entry::System(QUEUED_NOW.to_string()));
                self.model_mut(|model| model.queued.1 += 1);
                Cmd::Queue(input)
            }
            Some(_) => {
                self.push_entry(Entry::System(QUEUED_LATER.to_string()));
                Cmd::Queue(input)
            }
            None => Cmd::Submit(input),
        }
    }

    fn steer_queued(&self) {
        if matches!(self.model.get().busy_since, Some((Activity::Turn, _)))
            && self.model.get().queued.1 > 0
            && self.approval.peek().is_none()
            && self.form.peek().is_none()
        {
            drop(self.cmd.send(Cmd::SteerQueued));
        }
    }

    pub(in crate::tui::sigil_view) fn retry(&self) {
        if self.model.get().busy_since.is_some() {
            self.push_entry(Entry::System(
                "cancel the active turn before retrying".to_string(),
            ));
            self.refresh();
            return;
        }
        drop(self.cmd.send(Cmd::Command("/retry".to_string())));
    }

    pub(in crate::tui::sigil_view) fn complete(&self) -> bool {
        if self.settings_open.peek() {
            return false;
        }
        let typed = self.draft.peek();
        let Some(name) = self.commands.complete(
            &typed,
            self.caret.peek(),
            &self.model.get(),
            &self.picker.facts.peek(),
        ) else {
            return false;
        };
        self.completion(&name);
        true
    }

    pub(in crate::tui::sigil_view) fn accept(&self) -> bool {
        if self.settings_open.peek() {
            return false;
        }
        let typed = self.draft.peek();
        let Some((name, run)) = self
            .commands
            .accept(&typed, self.caret.peek(), &self.model.get())
        else {
            return false;
        };
        self.completion(&name);
        !run
    }

    pub(in crate::tui::sigil_view) fn escape_idle(&self) -> bool {
        if !armed(&self.escape_armed, crate::limits::limits().repeat_window()) {
            return false;
        }
        self.open_sessions(true);
        true
    }

    pub(in crate::tui::sigil_view) fn dismiss(&self) -> bool {
        self.commands
            .hide(&self.draft.peek(), self.caret.peek(), &self.model.get())
    }

    pub(in crate::tui::sigil_view) fn interrupt(&self) {
        if self.overlay_close() {
            return;
        }
        if let Some((activity, _)) = self.model.get().busy_since {
            if activity != Activity::Branch {
                drop(self.cmd.send(Cmd::Cancel));
            }
        } else if self.draft.peek().is_empty() && self.attachments.borrow().is_empty() {
            if armed(&self.quit_armed, crate::limits::limits().repeat_window()) {
                self.quit.set(true);
            } else {
                self.push_entry(Entry::System("press ctrl+c again to quit".to_string()));
                self.refresh();
            }
        } else {
            let draft = self.draft.peek();
            if !draft.trim().is_empty() {
                self.cleared.set(Some(draft));
            }
            self.replace(String::new());
            self.pasted_text.borrow_mut().clear();
            if !self.take_attachments().is_empty() {
                self.push_entry(Entry::System(
                    "draft discarded · attached images removed".to_string(),
                ));
                self.refresh();
            }
        }
    }
}
