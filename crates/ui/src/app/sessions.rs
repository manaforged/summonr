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
            summonr_agent::session::short(id)
        ))
    }

    pub(crate) fn open_session(&self, open: summonr_agent::hub::Open) {
        let open = if open == summonr_agent::hub::Open::New {
            self.sky.set(super::super::scene::fresh_sky());
            self.new_agent_place()
        } else {
            open
        };
        self.window.close_terminal();
        self.window.expect_worktree(&open);
        drop(self.cmd.send(Cmd::Open(open)));
    }

    /// Asks the window that holds the refused chat to let go of it.
    pub(crate) fn take_over(&self) {
        if let Some(id) = self.held.peek() {
            self.held.set(None);
            drop(self.cmd.send(Cmd::TakeOver(id)));
        }
    }

    fn new_agent_place(&self) -> summonr_agent::hub::Open {
        if crate::canvas::is_window() {
            summonr_agent::hub::Open::Worktree(self.workspace())
        } else {
            summonr_agent::hub::Open::New
        }
    }

    pub(crate) fn restore(&self, inputs: Vec<UserInput>) {
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

    pub(crate) fn delete_session(&self) {
        let rows = self.sessions.rows();
        let Some(info) = rows.get(self.sessions.list.at(rows.len())) else {
            return;
        };
        let again = crate::ui::alt_label('d');
        let note = self.forget(&info.id, &again).err().unwrap_or_default();
        self.sessions.note(&note);
    }

    pub(crate) fn resume_session(&self) {
        let rows = self.sessions.rows();
        if let Some(info) = rows.get(self.sessions.list.at(rows.len())) {
            if info.id != self.model.get().session {
                drop(self.cmd.send(Cmd::Command(format!("/resume {}", info.id))));
            }
            self.sessions.open.set(false);
        }
    }
}
