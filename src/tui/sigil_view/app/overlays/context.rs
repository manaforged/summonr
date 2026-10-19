use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn context_row(&self, index: usize) -> bool {
        let kind = self.layout.peek().secondary.unwrap_or(Pane::Files);
        let rows = match kind {
            Pane::Diff => self.diff_rows(),
            Pane::Worktrees => self.tree_rows(),
            Pane::Jobs => self.job_rows(),
            Pane::Mcp => self.mcp_rows(),
            Pane::Peers => self.peer_rows(),
            Pane::View => return false,
            _ => self.file_rows(),
        };
        let Some(target) = rows.get(index).cloned() else {
            return false;
        };
        match kind {
            Pane::Diff => self.diff_cursor.set(index),
            Pane::Worktrees => self.tree_cursor.set(index),
            Pane::Jobs => self.job_cursor.set(index),
            Pane::Mcp => self.mcp_cursor.set(index),
            Pane::Peers => self.peer_cursor.set(index),
            _ => self.file_cursor.set(index),
        }
        self.pane_focus.set(true);
        self.context.set(Some(Context {
            col: 0,
            row: 0,
            cursor: 0,
            target,
            kind,
            hunk: None,
        }));
        self.refresh();
        true
    }

    pub(in crate::tui::sigil_view) fn context_move(&self, delta: isize) {
        let Some(mut context) = self.context.peek() else {
            return;
        };
        let count = context.items().len() as isize;
        context.cursor = ((context.cursor as isize + delta).rem_euclid(count)) as usize;
        self.context.set(Some(context));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn context_close(&self) {
        self.context.set(None);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn context_enter(&self) {
        let Some(context) = self.context.peek() else {
            return;
        };
        self.context.set(None);
        let Some(action) = context
            .items()
            .get(context.cursor)
            .map(|(_, action)| *action)
        else {
            self.refresh();
            return;
        };
        match action {
            Action::Toggle => self.pane_enter(),
            Action::Hunks => self.diff_open(context.target.path.clone()),
            action @ (Action::Accept | Action::Reject | Action::Note) => {
                self.hunk_action(&context, action)
            }
            action @ (Action::Diff | Action::Merge | Action::Discard) => {
                self.tree_action(&context, action)
            }
            Action::Attach | Action::View | Action::Kill | Action::Open | Action::Insert => {
                self.row_action(&context, action)
            }
        }
    }

    fn hunk_action(&self, context: &Context, action: Action) {
        let Some(hunk) = context.hunk.clone() else {
            self.refresh();
            return;
        };
        match action {
            Action::Accept => self.hunk_toggle(&context.target.path, &hunk),
            Action::Reject => self.hunk_mark(&context.target.path, &hunk, crate::hunks::Op::Reject),
            Action::Note => self.note_start(context.target.path.clone(), hunk),
            action => unreachable!("context_enter routes only hunk actions: {action:?}"),
        }
    }

    fn tree_action(&self, context: &Context, action: Action) {
        match action {
            Action::Diff => self.tree_show(
                PathBuf::from(&context.target.path),
                context.target.name.clone(),
            ),
            Action::Merge => self.tree_merge(PathBuf::from(&context.target.path)),
            Action::Discard => self.tree_discard(PathBuf::from(&context.target.path)),
            action => unreachable!("context_enter routes only tree actions: {action:?}"),
        }
    }

    fn row_action(&self, context: &Context, action: Action) {
        match action {
            Action::Attach if context.kind == Pane::Peers => {
                self.peer_mention(&context.target.path)
            }
            Action::Attach => self.mention(&context.target.path),
            Action::View if context.kind == Pane::Mcp => self.mcp_enter(),
            Action::View if context.kind == Pane::Jobs => {
                let id = &context.target.path;
                let path = crate::jobs::load(&self.workspace, id).map_or_else(
                    |_| format!(".summonr/jobs/{id}.log"),
                    |record| {
                        crate::jobs::log_path(&self.workspace, &record)
                            .display()
                            .to_string()
                    },
                );
                self.view_open(path);
            }
            Action::View => self.view_open(context.target.path.clone()),
            Action::Kill => self.job_kill(&context.target.path),
            Action::Open => {
                self.open_file.set(Some(
                    self.root()
                        .unwrap_or_else(|| self.workspace.clone())
                        .join(&context.target.path),
                ));
                self.refresh();
            }
            Action::Insert => {
                let mut draft = self.draft.peek();
                draft.push_str(&context.target.path);
                self.replace(draft);
                self.pane_focus.set(false);
                self.refresh();
            }
            action => unreachable!("context_enter routes only row actions: {action:?}"),
        }
    }
}
