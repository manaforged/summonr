use super::*;

impl Overlay {
    const STACK: [Self; 12] = [
        Self::Help,
        Self::Search,
        Self::Sessions,
        Self::Usage,
        Self::Auth,
        Self::Model,
        Self::Effort,
        Self::Workflow,
        Self::Hotkeys,
        Self::Settings,
        Self::Context,
        Self::Cache,
    ];

    pub(in crate::tui::sigil_view) fn open(self, app: &App) -> Signal<bool> {
        match self {
            Self::Secret => app.secret_dialog,
            Self::Help => app.help.open,
            Self::Search => app.search.open,
            Self::Sessions => app.sessions.open,
            Self::Usage => app.usage.open,
            Self::Auth => app.auth.open,
            Self::Model => app.picker.open,
            Self::Effort => app.effort.open,
            Self::Workflow => app.workflow.open,
            Self::Hotkeys => app.hotkeys.open,
            Self::Settings => app.settings_open,
            Self::Context => app.context_open,
            Self::Cache => app.cache_open,
        }
    }

    fn keys(self) -> super::super::ui::Bindings {
        match self {
            Self::Sessions => super::super::sessions::keys(),
            Self::Auth => settings::auth_keys(),
            Self::Model => settings::model_picker_keys(),
            Self::Workflow => settings::workflow_keys(),
            Self::Hotkeys => settings::hotkeys_keys(),
            Self::Settings => settings::settings_keys(),
            Self::Secret
            | Self::Usage
            | Self::Help
            | Self::Search
            | Self::Effort
            | Self::Context
            | Self::Cache => Vec::new(),
        }
    }

    pub(in crate::tui::sigil_view) fn layer(self, app: App) -> Option<sigil::prelude::NodeBuilder> {
        let keys = self.keys();
        let owner = app.clone();
        let node = match self {
            Self::Secret => super::super::secret::layer(app),
            Self::Help => super::super::help::layer(app),
            Self::Search => super::super::search::layer(app),
            Self::Sessions => super::super::sessions::layer(app),
            Self::Usage => super::super::usage::layer(app),
            Self::Auth if app.picker.open.peek() => settings::model_picker_layer(app),
            Self::Auth => settings::auth_layer(app),
            Self::Model => settings::model_picker_layer(app),
            Self::Workflow => settings::workflow_layer(app),
            Self::Hotkeys => settings::hotkeys_layer(app),
            Self::Settings => settings::settings_layer(app),
            Self::Effort | Self::Context | Self::Cache => return None,
        };
        Some(super::super::ui::keyed(node, &owner, keys))
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn overlay(&self) -> Option<Overlay> {
        if self.secret_dialog.get() {
            return Some(Overlay::Secret);
        }
        Overlay::STACK
            .into_iter()
            .find(|overlay| overlay.open(self).get())
    }

    pub(in crate::tui::sigil_view) fn options_active(&self) -> bool {
        self.overlay().is_some()
    }

    pub(in crate::tui::sigil_view) fn overlay_move(&self, delta: isize) -> bool {
        match self.overlay() {
            Some(Overlay::Help) => crate::tui::sigil_view::help::shift(self, delta),
            Some(Overlay::Search) => self.search.shift(&self.model.get(), delta),
            Some(Overlay::Sessions) => self.sessions.shift(delta),
            Some(Overlay::Auth) => self.move_auth(delta),
            Some(Overlay::Model) => self.move_model(delta),
            Some(Overlay::Effort) => self.effort.shift(&self.model.get().snapshot.levels, delta),
            Some(Overlay::Workflow) => self.move_workflow(delta),
            Some(Overlay::Hotkeys) => self.move_hotkey(delta),
            Some(Overlay::Settings) => self.move_setting(delta),
            Some(Overlay::Usage) => self.usage.shift(delta),
            Some(Overlay::Secret | Overlay::Context | Overlay::Cache) => {}
            None => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_tab(&self, delta: isize) -> bool {
        match self.overlay() {
            Some(Overlay::Sessions) => self.sessions.scope(),
            Some(Overlay::Usage) => self.usage.metric(),
            Some(Overlay::Model) => self.cycle_facet(delta),
            Some(_) | None => return self.overlay_move(delta),
        }
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_enter(&self) -> bool {
        match self.overlay() {
            Some(Overlay::Secret) => self.secret_finish(true),
            Some(Overlay::Help) => return true,
            Some(Overlay::Search) => self.jump(),
            Some(Overlay::Sessions) => self.resume_session(),
            Some(Overlay::Auth) => self.commit_auth(),
            Some(Overlay::Model) => self.commit_model_edit(),
            Some(Overlay::Effort) => self.commit_effort(),
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
            Some(Overlay::Secret) => self.secret_finish(false),
            Some(Overlay::Help) => self.help.open.set(false),
            Some(Overlay::Search) => self.search.open.set(false),
            Some(Overlay::Sessions) => self.sessions.open.set(false),
            Some(Overlay::Auth) => self.close_auth(),
            Some(Overlay::Model) => self.close_model(),
            Some(Overlay::Effort) => self.effort.open.set(false),
            Some(Overlay::Workflow) => self.close_workflow(),
            Some(Overlay::Hotkeys) => self.close_hotkey(),
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

    fn close_auth(&self) {
        self.cancel_auth();
    }

    fn close_model(&self) {
        self.cancel_model_edit();
    }

    fn close_hotkey(&self) {
        if self.hotkeys.recording.peek() {
            self.hotkeys.recording.set(false);
            self.hotkeys.note.set(String::new());
        } else {
            self.close_hotkeys();
        }
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
            Some(Overlay::Workflow) if !self.workflow.naming.peek() => self.cycle_workflow(delta),
            Some(Overlay::Settings)
                if self.selected_setting() == Some(super::settings::Setting::Endpoint) =>
            {
                self.cycle_seat(delta >= 0);
            }
            Some(Overlay::Settings) => self.adjust_setting(),
            Some(Overlay::Model | Overlay::Auth | Overlay::Usage) => return self.step_tab(delta),
            Some(Overlay::Effort) => self.effort.shift(&self.model.get().snapshot.levels, delta),
            _ => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn workflow_keys(&self) -> bool {
        self.overlay() == Some(Overlay::Workflow) && !self.workflow.naming.peek()
    }

    pub(in crate::tui::sigil_view) fn copy_seat(&self, from: usize, to: usize) {
        let Some(role) = Role::ALL.get(to) else {
            return;
        };
        let seat = self.workflow.seats.peek().get(from).cloned().flatten();
        let command = match seat.and_then(|seat| {
            seat.split_once('\t')
                .map(|(endpoint, model)| (endpoint.to_string(), model.to_string()))
        }) {
            Some((endpoint, model)) => Cmd::SeatRole(*role, endpoint, model),
            None => Cmd::ClearRole(*role),
        };
        self.workflow.sel.set(to);
        drop(self.cmd.send(command));
    }

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
            Action::Merge => {
                drop(
                    self.cmd
                        .send(Cmd::Merge(PathBuf::from(&context.target.path))),
                );
                self.refresh();
            }
            Action::Discard => {
                drop(
                    self.cmd
                        .send(Cmd::Discard(PathBuf::from(&context.target.path))),
                );
                self.refresh();
            }
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
