use super::*;

impl App {
    fn forget(&self, id: &str, again: &str) -> Result<(), String> {
        if id == self.model.get().session {
            return Err("the open session cannot be deleted".to_string());
        }
        if confirm(&self.sessions.armed, id.to_string()) {
            drop(self.cmd.send(Cmd::DeleteSession(id.to_string())));
            return Ok(());
        }
        Err(format!(
            "{again} again deletes {}",
            crate::session::short(id)
        ))
    }

    pub(in crate::tui::sigil_view) fn open_session(&self, open: crate::tui::hub::Open) {
        if open == crate::tui::hub::Open::New {
            self.sky.set(super::super::scene::fresh_sky());
        }
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
            "pending input restored; edit it, then enter sends".to_string(),
        ));
    }

    pub(in crate::tui::sigil_view) fn delete_session(&self) {
        let rows = self.sessions.rows();
        let Some(info) = rows.get(self.sessions.list.at(rows.len())) else {
            return;
        };
        let again = crate::tui::sigil_view::ui::alt_label('d');
        let note = self.forget(&info.id, &again).err().unwrap_or_default();
        self.sessions.note(&note);
    }

    pub(in crate::tui::sigil_view) fn resume_session(&self) {
        let rows = self.sessions.rows();
        if let Some(info) = rows.get(self.sessions.list.at(rows.len())) {
            if info.id != self.model.get().session {
                drop(self.cmd.send(Cmd::Command(format!("/resume {}", info.id))));
            }
            self.sessions.open.set(false);
        }
    }
}
