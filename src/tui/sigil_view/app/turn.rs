use super::*;

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
        if self.model.peek().snapshot.unseated {
            self.ask_provider();
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
        if let Some(line) = crate::command::alias(&text) {
            text = line;
        }
        text
    }

    fn dispatch_line(&self, text: &str) -> bool {
        let argument = match argument(text, &self.workspace) {
            Ok(argument) => argument,
            Err(error) => {
                self.cleared.set(Some(text.to_owned()));
                self.replace(String::new());
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
        if crate::parse_command(text).is_some() {
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
            Some(Activity::Turn)
                if self.approval.peek().is_none() && self.form.peek().is_none() =>
            {
                Cmd::Steer(input)
            }
            Some(_) => {
                self.push_entry(Entry::System(crate::tui::worker::QUEUED_LATER.to_string()));
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
        if run {
            self.submit();
        }
        true
    }

    pub(in crate::tui::sigil_view) fn escape_idle(&self) -> bool {
        if !armed(&self.escape_armed, crate::limits::limits().repeat_window()) {
            return false;
        }
        self.open_sessions(true);
        true
    }

    pub(in crate::tui::sigil_view) fn dismiss(&self) -> bool {
        let hidden = self
            .commands
            .hide(&self.draft.peek(), self.caret.peek(), &self.model.get());
        if hidden && crate::tui::sigil_view::canvas::is_window() {
            self.drop_bare_trigger();
        }
        hidden
    }

    fn drop_bare_trigger(&self) {
        let draft = self.draft.peek();
        let caret = self.caret.peek().min(draft.len());
        let (before, after) = draft.split_at(caret);
        let stripped = before.strip_suffix('@').or_else(|| {
            before
                .strip_suffix('/')
                .filter(|rest| rest.trim().is_empty())
        });
        let Some(stripped) = stripped else {
            return;
        };
        if !stripped.is_empty() && !stripped.ends_with(char::is_whitespace) {
            return;
        }
        let kept = stripped.trim_end_matches(' ');
        let next = format!("{kept}{after}");
        self.replace(next);
        self.caret.set(kept.len());
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
