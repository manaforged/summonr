use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn request_resume(&self, id: String) {
        if !self.draft.peek().is_empty() || !self.attachments.borrow().is_empty() {
            self.model.borrow_mut().entries.push(Entry::Error(
                "Draft kept. Send it or press Ctrl+C to discard it before switching sessions."
                    .to_string(),
            ));
            self.refresh();
            return;
        }
        *self.resume.borrow_mut() = Some(id);
        self.quit.set(true);
    }

    pub(in crate::tui::sigil_view) fn restore(&self, inputs: Vec<UserInput>) {
        if inputs.is_empty() {
            return;
        }
        let draft = self.draft.peek();
        let caret = self.caret.peek();
        let mut prompts = Vec::new();
        let mut pending = Vec::new();
        for input in inputs {
            if !input.text.is_empty() {
                prompts.push(input.text);
            }
            pending.extend(input.images.into_iter().map(Attachment::restore));
        }
        pending.extend(self.take_attachments());
        self.attached.set(pending.len());
        *self.attachments.borrow_mut() = pending;
        if !draft.is_empty() {
            prompts.push(draft.clone());
        }
        let text = prompts.join("\n\n");
        let position = text.len().saturating_sub(draft.len()) + caret;
        self.replace(text);
        self.caret.set(position);
        self.model.borrow_mut().entries.push(Entry::System(
            "Pending input restored. Edit the composer, then press Enter to send.".to_string(),
        ));
    }

    pub(in crate::tui::sigil_view) fn delete_session(&self) {
        let rows = self.sessions.rows();
        let Some(info) = rows.get(
            self.sessions
                .selected
                .peek()
                .min(rows.len().saturating_sub(1)),
        ) else {
            return;
        };
        if info.id == self.model.borrow().session {
            self.sessions.note("the open session cannot be deleted");
            return;
        }
        if armed(&self.delete_armed, REPEAT_WINDOW) {
            drop(self.cmd.send(Cmd::DeleteSession(info.id.clone())));
            self.sessions.note("");
        } else {
            self.sessions.note(&format!(
                "ctrl+d again deletes {}",
                crate::session::short(&info.id)
            ));
        }
    }

    pub(in crate::tui::sigil_view) fn resume_session(&self) {
        let rows = self.sessions.rows();
        if let Some(info) = rows.get(
            self.sessions
                .selected
                .peek()
                .min(rows.len().saturating_sub(1)),
        ) {
            if info.id != self.model.borrow().session {
                drop(self.cmd.send(Cmd::Command(format!("/resume {}", info.id))));
            }
            self.sessions.open.set(false);
        }
    }
}
