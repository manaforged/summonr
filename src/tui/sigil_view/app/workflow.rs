use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn open_workflow(&self) {
        self.workflow_sel.set(0);
        self.workflow_naming.set(false);
        self.replace_workflow_draft(String::new());
        self.workflow_open.set(true);
        drop(self.cmd.send(Cmd::ListWorkflows));
    }

    pub(in crate::tui::sigil_view) fn close_workflow(&self) {
        if self.workflow_naming.peek() {
            self.workflow_naming.set(false);
            self.replace_workflow_draft(String::new());
        } else {
            self.workflow_open.set(false);
        }
    }

    pub(in crate::tui::sigil_view) fn move_workflow(&self, delta: isize) {
        if self.workflow_naming.peek() {
            return;
        }
        let last = Role::ALL.len().saturating_sub(1);
        self.workflow_sel.set(
            self.workflow_sel
                .peek()
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(in crate::tui::sigil_view) fn cycle_workflow(&self, delta: isize) {
        if self.workflow_naming.peek() {
            return;
        }
        let names = self.workflow_names.peek();
        if names.is_empty() {
            return;
        }
        let current = self.workflow_name.peek();
        let index = names.iter().position(|name| *name == current);
        let next = match (index, delta.is_negative()) {
            (Some(index), _) => (index as isize + delta).rem_euclid(names.len() as isize) as usize,
            (None, true) => names.len() - 1,
            (None, false) => 0,
        };
        drop(
            self.cmd
                .send(Cmd::Command(format!("/workflow {}", names[next]))),
        );
    }

    pub(in crate::tui::sigil_view) fn pick_workflow_seat(&self) {
        if self.workflow_naming.peek() {
            self.commit_workflow();
            return;
        }
        self.model_for.set(Some(self.workflow_sel.peek()));
        self.begin_model_edit();
    }

    pub(in crate::tui::sigil_view) fn clear_workflow_seat(&self) {
        if self.workflow_naming.peek() {
            return;
        }
        if let Some(role) = Role::ALL.get(self.workflow_sel.peek()) {
            drop(self.cmd.send(Cmd::ClearRole(*role)));
        }
    }

    pub(in crate::tui::sigil_view) fn begin_workflow_save(&self) {
        if self.workflow_naming.peek() {
            return;
        }
        self.replace_workflow_draft(self.workflow_name.peek());
        self.workflow_naming.set(true);
    }

    pub(in crate::tui::sigil_view) fn commit_workflow(&self) {
        let name = self.workflow_draft.peek().trim().to_string();
        if name.is_empty() {
            return;
        }
        self.workflow_naming.set(false);
        self.replace_workflow_draft(String::new());
        drop(self.cmd.send(Cmd::SaveWorkflow(name)));
    }

    pub(in crate::tui::sigil_view) fn replace_workflow_draft(&self, draft: String) {
        self.workflow_caret.set(draft.len());
        self.workflow_draft.set(draft);
    }
}
