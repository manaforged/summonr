use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn open_workflow(&self) {
        self.workflow.sel.set(0);
        self.workflow.naming.set(false);
        self.replace_workflow_draft(String::new());
        self.workflow.open.set(true);
        drop(self.cmd.send(Cmd::ListWorkflows));
    }

    pub(in crate::tui::sigil_view) fn close_workflow(&self) {
        if self.workflow.naming.peek() {
            self.workflow.naming.set(false);
            self.replace_workflow_draft(String::new());
        } else {
            self.workflow.open.set(false);
        }
    }

    pub(in crate::tui::sigil_view) fn move_workflow(&self, delta: isize) {
        if self.workflow.naming.peek() {
            return;
        }
        let last = Role::ALL.len().saturating_sub(1);
        self.workflow.sel.set(
            self.workflow
                .sel
                .peek()
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(in crate::tui::sigil_view) fn cycle_workflow(&self, delta: isize) {
        if self.workflow.naming.peek() {
            return;
        }
        let names = self.workflow.names.peek();
        if names.is_empty() {
            return;
        }
        let current = self.workflow.name.peek();
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
        if self.workflow.naming.peek() {
            self.commit_workflow();
            return;
        }
        self.picker.target.set(Some(self.workflow.sel.peek()));
        self.begin_model_edit();
    }

    pub(in crate::tui::sigil_view) fn clear_workflow_seat(&self) {
        if self.workflow.naming.peek() {
            return;
        }
        if let Some(role) = Role::ALL.get(self.workflow.sel.peek()) {
            drop(self.cmd.send(Cmd::ClearRole(*role)));
        }
    }

    pub(in crate::tui::sigil_view) fn begin_workflow_save(&self) {
        if self.workflow.naming.peek() {
            return;
        }
        self.replace_workflow_draft(self.workflow.name.peek());
        self.workflow.naming.set(true);
    }

    pub(in crate::tui::sigil_view) fn commit_workflow(&self) {
        let name = self.workflow.draft.peek().trim().to_string();
        if name.is_empty() {
            return;
        }
        self.workflow.naming.set(false);
        self.replace_workflow_draft(String::new());
        drop(self.cmd.send(Cmd::SaveWorkflow(name)));
    }

    pub(in crate::tui::sigil_view) fn replace_workflow_draft(&self, draft: String) {
        self.workflow.caret.set(draft.len());
        self.workflow.draft.set(draft);
    }
}
