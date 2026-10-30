use sigil::prelude::NodeBuilder;

use super::super::ui::Bindings;
use super::*;

pub(super) mod context;

struct Row {
    overlay: Overlay,
    open: fn(&App) -> Signal<bool>,
    keys: fn() -> Bindings,
    node: Option<fn(App) -> NodeBuilder>,
    shift: fn(&App, isize),
    tab: Option<fn(&App, isize)>,
    adjust: fn(&App, isize) -> bool,
    viewport: Option<fn(&App) -> Signal<f32>>,
    enter: fn(&App),
    close: Option<fn(&App)>,
    dismiss: fn(&App),
}

const ROW: Row = Row {
    overlay: Overlay::Secret,
    open: |app| app.secret_dialog,
    keys: Vec::new,
    node: None,
    shift: |_, _| {},
    tab: None,
    adjust: |_, _| false,
    viewport: None,
    enter: |_| {},
    close: None,
    dismiss: |app| {
        app.overlay_close();
    },
};

static ROWS: [Row; 13] = [
    Row {
        overlay: Overlay::Secret,
        open: |app| app.secret_dialog,
        node: Some(super::super::secret::layer),
        enter: |app| app.secret_finish(true),
        close: Some(|app| app.secret_finish(false)),
        ..ROW
    },
    Row {
        overlay: Overlay::Help,
        open: |app| app.help.open,
        node: Some(super::super::help::layer),
        shift: super::super::help::shift,
        ..ROW
    },
    Row {
        overlay: Overlay::Search,
        open: |app| app.search.open,
        node: Some(super::super::search::layer),
        shift: |app, delta| app.search.shift(&app.model.get(), delta),
        enter: App::jump,
        ..ROW
    },
    Row {
        overlay: Overlay::Sessions,
        open: |app| app.sessions.open,
        keys: super::super::sessions::keys,
        node: Some(super::super::sessions::layer),
        shift: |app, delta| app.sessions.shift(delta),
        tab: Some(|app, _| app.sessions.scope()),
        enter: App::resume_session,
        ..ROW
    },
    Row {
        overlay: Overlay::Usage,
        open: |app| app.usage.open,
        node: Some(super::super::usage::layer),
        shift: |app, delta| app.usage.shift(delta),
        tab: Some(|app, _| app.usage.metric()),
        adjust: App::step_tab,
        enter: |app| app.usage.scope(),
        ..ROW
    },
    Row {
        overlay: Overlay::Auth,
        open: |app| app.auth.open,
        keys: settings::auth_keys,
        node: Some(|app| {
            if app.picker.open.peek() {
                settings::model_picker_layer(app)
            } else {
                settings::auth_layer(app)
            }
        }),
        shift: App::move_auth,
        adjust: App::step_tab,
        viewport: Some(|app| {
            if app.picker.open.peek() {
                app.picker.list.scroll
            } else {
                app.auth.list.scroll
            }
        }),
        enter: App::commit_auth,
        close: Some(App::cancel_auth),
        dismiss: |app| {
            if app.picker.open.peek() {
                app.overlay_close();
            } else {
                app.dismiss_auth();
            }
        },
        ..ROW
    },
    Row {
        overlay: Overlay::Model,
        open: |app| app.picker.open,
        keys: settings::model_picker_keys,
        node: Some(settings::model_picker_layer),
        shift: App::move_model,
        tab: Some(App::cycle_facet),
        adjust: App::step_tab,
        viewport: Some(|app| app.picker.list.scroll),
        enter: App::commit_model_edit,
        close: Some(App::cancel_model_edit),
        ..ROW
    },
    Row {
        overlay: Overlay::Effort,
        open: |app| app.effort.open,
        shift: App::shift_effort,
        adjust: |app, delta| {
            app.shift_effort(delta);
            true
        },
        enter: App::commit_effort,
        ..ROW
    },
    Row {
        overlay: Overlay::Workflow,
        open: |app| app.workflow.open,
        keys: settings::workflow_keys,
        node: Some(settings::workflow_layer),
        shift: App::move_workflow,
        adjust: |app, delta| {
            let cycle = !app.workflow.naming.peek();
            if cycle {
                app.cycle_workflow(delta);
            }
            cycle
        },
        enter: App::pick_workflow_model,
        close: Some(App::close_workflow),
        ..ROW
    },
    Row {
        overlay: Overlay::Hotkeys,
        open: |app| app.hotkeys.open,
        keys: settings::hotkeys_keys,
        node: Some(settings::hotkeys_layer),
        shift: App::move_hotkey,
        enter: App::edit_hotkey,
        close: Some(App::close_hotkey),
        ..ROW
    },
    Row {
        overlay: Overlay::Settings,
        open: |app| app.settings_open,
        keys: settings::settings_keys,
        node: Some(settings::settings_layer),
        shift: App::move_setting,
        viewport: Some(|app| app.settings.scroll),
        adjust: |app, delta| {
            if app.selected_setting() == Some(super::settings::Setting::Endpoint) {
                app.cycle_model(delta >= 0);
            } else {
                app.adjust_setting();
            }
            true
        },
        enter: App::adjust_setting,
        ..ROW
    },
    Row {
        overlay: Overlay::Context,
        open: |app| app.context_open,
        enter: App::close_meters,
        close: Some(App::close_meters),
        ..ROW
    },
    Row {
        overlay: Overlay::Cache,
        open: |app| app.cache_open,
        enter: App::close_meters,
        close: Some(App::close_meters),
        ..ROW
    },
];

impl Overlay {
    fn row(self) -> &'static Row {
        &ROWS[self as usize]
    }

    pub(in crate::tui::sigil_view) fn layer(self, app: App) -> Option<NodeBuilder> {
        let row = self.row();
        let node = row.node?;
        let owner = app.clone();
        let (closer, dismiss) = (app.clone(), row.dismiss);
        let close: std::rc::Rc<dyn Fn()> = std::rc::Rc::new(move || dismiss(&closer));
        let node = super::super::ui::dismissing(close, || node(app));
        Some(super::super::ui::keyed(node, &owner, (row.keys)()))
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn overlay(&self) -> Option<Overlay> {
        ROWS.iter()
            .find(|row| (row.open)(self).get())
            .map(|row| row.overlay)
    }

    pub(in crate::tui::sigil_view) fn covered(&self) -> bool {
        self.overlay()
            .is_some_and(|overlay| overlay.row().node.is_some())
    }

    pub(in crate::tui::sigil_view) fn options_active(&self) -> bool {
        self.overlay().is_some()
    }

    pub(in crate::tui::sigil_view) fn overlay_move(&self, delta: isize) -> bool {
        let Some(overlay) = self.overlay() else {
            return false;
        };
        (overlay.row().shift)(self, delta);
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_scroll(&self, lines: isize) -> bool {
        if !super::super::canvas::is_window() {
            return self.overlay_move(lines);
        }
        let Some(viewport) = self.overlay().and_then(|overlay| overlay.row().viewport) else {
            return self.overlay_move(lines);
        };
        let viewport = viewport(self);
        let step = lines as f32 * super::super::canvas::overlay_list_row();
        viewport.set((viewport.peek() + step).max(0.0));
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_tab(&self, delta: isize) -> bool {
        let Some(tab) = self.overlay().and_then(|overlay| overlay.row().tab) else {
            return self.overlay_move(delta);
        };
        tab(self, delta);
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_enter(&self) -> bool {
        let Some(overlay) = self.overlay() else {
            return false;
        };
        (overlay.row().enter)(self);
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_close(&self) -> bool {
        self.scroll.set(0);
        let Some(overlay) = self.overlay() else {
            return false;
        };
        let row = overlay.row();
        match row.close {
            Some(close) => close(self),
            None => (row.open)(self).set(false),
        }
        true
    }

    pub(in crate::tui::sigil_view) fn overlay_adjust(&self, delta: isize) -> bool {
        self.overlay()
            .is_some_and(|overlay| (overlay.row().adjust)(self, delta))
    }

    fn shift_effort(&self, delta: isize) {
        self.effort.shift(&self.model.get().snapshot.levels, delta);
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

    pub(in crate::tui::sigil_view) fn workflow_keys(&self) -> bool {
        self.overlay() == Some(Overlay::Workflow) && !self.workflow.naming.peek()
    }

    pub(in crate::tui::sigil_view) fn copy_model(&self, from: usize, to: usize) {
        let Some(role) = Role::ALL.get(to) else {
            return;
        };
        let selection = self.workflow.selections.peek().get(from).cloned().flatten();
        let command = match selection.and_then(|selection| {
            selection
                .split_once('\t')
                .map(|(endpoint, model)| (endpoint.to_string(), model.to_string()))
        }) {
            Some((endpoint, model)) => Cmd::AssignModel(*role, endpoint, model),
            None => Cmd::ClearRole(*role),
        };
        self.workflow.list.cursor.set(to);
        drop(self.cmd.send(command));
    }
}
