use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn overlay(&self) -> Option<Overlay> {
        if self.help.open.get() {
            Some(Overlay::Help)
        } else if self.search.open.get() {
            Some(Overlay::Search)
        } else if self.sessions.open.get() {
            Some(Overlay::Sessions)
        } else if self.usage.open.get() {
            Some(Overlay::Usage)
        } else if self.auth_open.get() {
            Some(Overlay::Auth)
        } else if self.editing_model.get() {
            Some(Overlay::Model)
        } else if self.workflow_open.get() {
            Some(Overlay::Workflow)
        } else if self.hotkeys_open.get() {
            Some(Overlay::Hotkeys)
        } else if self.settings_open.get() {
            Some(Overlay::Settings)
        } else if self.context_open.get() {
            Some(Overlay::Context)
        } else if self.cache_open.get() {
            Some(Overlay::Cache)
        } else {
            None
        }
    }

    pub(in crate::tui::sigil_view) fn options_active(&self) -> bool {
        self.overlay().is_some()
    }

    pub(in crate::tui::sigil_view) fn overlay_move(&self, delta: isize) -> bool {
        match self.overlay() {
            Some(Overlay::Help) => {}
            Some(Overlay::Search) => self.search.shift(&self.model.get(), delta),
            Some(Overlay::Sessions) => self.sessions.shift(delta),
            Some(Overlay::Auth) => self.move_auth(delta),
            Some(Overlay::Model) => self.move_model(delta),
            Some(Overlay::Workflow) => self.move_workflow(delta),
            Some(Overlay::Hotkeys) => self.move_hotkey(delta),
            Some(Overlay::Settings) => self.move_setting(delta),
            Some(Overlay::Context | Overlay::Cache | Overlay::Usage) => {}
            None => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_tab(&self, delta: isize) -> bool {
        if self.overlay() == Some(Overlay::Sessions) {
            self.sessions.scope();
            return true;
        }
        if self.overlay() == Some(Overlay::Usage) {
            self.usage.metric();
            return true;
        }
        self.overlay_move(delta)
    }

    pub(in crate::tui::sigil_view) fn overlay_enter(&self) -> bool {
        match self.overlay() {
            Some(Overlay::Help) => return true,
            Some(Overlay::Search) => self.jump(),
            Some(Overlay::Sessions) => self.resume_session(),
            Some(Overlay::Auth) => self.commit_auth(),
            Some(Overlay::Model) => self.commit_model_edit(),
            Some(Overlay::Workflow) => self.pick_workflow_seat(),
            Some(Overlay::Hotkeys) => self.edit_hotkey(),
            Some(Overlay::Settings) => self.adjust_setting(),
            Some(Overlay::Usage) => self.usage.scope(),
            Some(Overlay::Context | Overlay::Cache) => {
                self.context_open.set(false);
                self.cache_open.set(false);
            }
            None => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_close(&self) -> bool {
        self.scroll.set(0);
        match self.overlay() {
            Some(Overlay::Help) => self.help.open.set(false),
            Some(Overlay::Search) => self.search.open.set(false),
            Some(Overlay::Sessions) => self.sessions.open.set(false),
            Some(Overlay::Auth)
                if matches!(self.auth_step.peek(), Login::List(_))
                    && !self.auth_filter.peek().is_empty() =>
            {
                self.replace_auth_filter(String::new());
            }
            Some(Overlay::Auth) => self.cancel_auth(),
            Some(Overlay::Model) if !self.model_draft.peek().is_empty() => {
                self.replace_model_draft(String::new());
            }
            Some(Overlay::Model) => self.cancel_model_edit(),
            Some(Overlay::Workflow) => self.close_workflow(),
            Some(Overlay::Hotkeys) if self.hotkey_recording.peek() => {
                self.hotkey_recording.set(false);
                self.hotkey_note.set(String::new());
            }
            Some(Overlay::Hotkeys) => self.close_hotkeys(),
            Some(Overlay::Settings) => self.settings_open.set(false),
            Some(Overlay::Usage) => self.usage.open.set(false),
            Some(Overlay::Context | Overlay::Cache) => {
                self.context_open.set(false);
                self.cache_open.set(false);
            }
            None => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn toggle_meter(&self, target: super::chat::Target) {
        match target {
            super::chat::Target::Context => {
                let open = !self.context_open.peek();
                self.context_open.set(open);
                if open {
                    self.cache_open.set(false);
                }
            }
            super::chat::Target::Cache => {
                let open = !self.cache_open.peek();
                self.cache_open.set(open);
                if open {
                    self.context_open.set(false);
                }
            }
        }
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn overlay_adjust(&self, delta: isize) -> bool {
        match self.overlay() {
            Some(Overlay::Workflow) if !self.workflow_naming.peek() => self.cycle_workflow(delta),
            Some(Overlay::Settings)
                if self.selected_setting() == Some(super::settings::Setting::Endpoint) =>
            {
                self.cycle_seat(delta >= 0);
            }
            Some(Overlay::Settings) => self.adjust_setting(),
            Some(Overlay::Usage) => self.usage.shift(delta),
            _ => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn workflow_keys(&self) -> bool {
        self.overlay() == Some(Overlay::Workflow) && !self.workflow_naming.peek()
    }

    pub(in crate::tui::sigil_view) fn copy_seat(&self, from: usize, to: usize) {
        let Some(role) = Role::ALL.get(to) else {
            return;
        };
        let seat = self.workflow_seats.peek().get(from).cloned().flatten();
        let command = match seat.and_then(|seat| {
            seat.split_once('\t')
                .map(|(endpoint, model)| (endpoint.to_string(), model.to_string()))
        }) {
            Some((endpoint, model)) => Cmd::SeatRole(*role, endpoint, model),
            None => Cmd::ClearRole(*role),
        };
        self.workflow_sel.set(to);
        drop(self.cmd.send(command));
    }

    pub(in crate::tui::sigil_view) fn context_row(&self, index: usize) -> bool {
        let kind = self.layout.peek().secondary.unwrap_or(Pane::Files);
        let rows = match kind {
            Pane::Diff => self.diff_rows(),
            Pane::Worktrees => self.tree_rows(),
            Pane::View => return false,
            _ => self.file_rows(),
        };
        let Some(target) = rows.get(index).cloned() else {
            return false;
        };
        match kind {
            Pane::Diff => self.diff_cursor.set(index),
            Pane::Worktrees => self.tree_cursor.set(index),
            _ => self.file_cursor.set(index),
        }
        self.pane_focus.set(true);
        self.context.set(Some(Context {
            col: 0,
            row: 0,
            cursor: 0,
            target,
            kind,
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
        match context
            .items()
            .get(context.cursor)
            .map(|(_, action)| *action)
        {
            Some(Action::Toggle) => self.pane_enter(),
            Some(Action::Attach) => self.mention(&context.target.path),
            Some(Action::Hunks) => self.diff_open(context.target.path.clone()),
            Some(Action::View) => self.view_open(context.target.path.clone()),
            Some(Action::Diff) => {
                self.tree_show(
                    PathBuf::from(&context.target.path),
                    context.target.name.clone(),
                );
            }
            Some(Action::Merge) => {
                drop(
                    self.cmd
                        .send(Cmd::Merge(PathBuf::from(&context.target.path))),
                );
                self.refresh();
            }
            Some(Action::Discard) => {
                drop(
                    self.cmd
                        .send(Cmd::Discard(PathBuf::from(&context.target.path))),
                );
                self.refresh();
            }
            Some(Action::Open) => {
                self.open_file.set(Some(
                    self.root()
                        .unwrap_or_else(|| self.workspace.clone())
                        .join(&context.target.path),
                ));
                self.refresh();
            }
            Some(Action::Insert) => {
                let mut draft = self.draft.peek();
                draft.push_str(&context.target.path);
                self.replace(draft);
                self.pane_focus.set(false);
                self.refresh();
            }
            None => self.refresh(),
        }
    }
}
