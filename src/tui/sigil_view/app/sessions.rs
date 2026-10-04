use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn open_session(&self, open: crate::tui::hub::Open) {
        drop(self.cmd.send(Cmd::Open(open)));
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
        self.push_entry(Entry::System(
            "pending input restored — edit it, then enter sends".to_string(),
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
        if info.id == self.model.get().session {
            self.sessions.note("the open session cannot be deleted");
            return;
        }
        if armed(&self.delete_armed, REPEAT_WINDOW) {
            drop(self.cmd.send(Cmd::DeleteSession(info.id.clone())));
            self.sessions.note("");
        } else {
            self.sessions.note(&format!(
                "alt+d again deletes {}",
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
            if info.id != self.model.get().session {
                drop(self.cmd.send(Cmd::Command(format!("/resume {}", info.id))));
            }
            self.sessions.open.set(false);
        }
    }
}
