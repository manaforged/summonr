use super::*;

mod context;

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

    const fn modal(self) -> bool {
        !matches!(self, Self::Effort | Self::Context | Self::Cache)
    }

    pub(in crate::tui::sigil_view) fn layer(self, app: App) -> Option<sigil::prelude::NodeBuilder> {
        if !self.modal() {
            return None;
        }
        let keys = self.keys();
        let owner = app.clone();
        let closer = app.clone();
        let close: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || {
            if closer.overlay() == Some(Self::Auth) && !closer.picker.open.peek() {
                closer.dismiss_auth();
            } else {
                closer.overlay_close();
            }
        });
        let node = super::super::ui::dismissing(close, || self.node(app))?;
        Some(super::super::ui::keyed(node, &owner, keys))
    }

    fn node(self, app: App) -> Option<sigil::prelude::NodeBuilder> {
        Some(match self {
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
        })
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

    pub(in crate::tui::sigil_view) fn covered(&self) -> bool {
        self.overlay().is_some_and(Overlay::modal)
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

    pub(in crate::tui::sigil_view) fn overlay_scroll(&self, lines: isize) -> bool {
        if !super::super::canvas::is_window() {
            return self.overlay_move(lines);
        }
        let viewport = match self.overlay() {
            Some(Overlay::Auth) if !self.picker.open.peek() => self.auth.scroll,
            Some(Overlay::Auth | Overlay::Model) => self.picker.scroll,
            Some(_) | None => return self.overlay_move(lines),
        };
        let step = lines as f32 * super::super::canvas::overlay_list_row();
        viewport.set((viewport.peek() + step).max(0.0));
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
            Some(Overlay::Context | Overlay::Cache) => self.close_meters(),
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
            Some(Overlay::Auth) => self.cancel_auth(),
            Some(Overlay::Model) => self.cancel_model_edit(),
            Some(Overlay::Effort) => self.effort.open.set(false),
            Some(Overlay::Workflow) => self.close_workflow(),
            Some(Overlay::Hotkeys) => self.close_hotkey(),
            Some(Overlay::Settings) => self.settings_open.set(false),
            Some(Overlay::Usage) => self.usage.open.set(false),
            Some(Overlay::Context | Overlay::Cache) => self.close_meters(),
            None => return false,
        }
        true
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

    pub(in crate::tui::sigil_view) fn close_meters(&self) {
        if self.context_open.peek() || self.cache_open.peek() {
            self.context_open.set(false);
            self.cache_open.set(false);
        }
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
}
