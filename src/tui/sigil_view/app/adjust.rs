use super::*;

const AUTO_COMMAND: &str = "/auto";
const GOAL_PREFILL: &str = "/goal ";

mod display;

impl App {
    pub(in crate::tui::sigil_view) fn move_setting(&self, delta: isize) {
        if self.picker.open.peek() {
            return;
        }
        let last = super::settings::setting_rows(self).len().saturating_sub(1);
        self.settings_sel.set(
            self.settings_sel
                .peek()
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(in crate::tui::sigil_view) fn selected_setting(&self) -> Option<super::settings::Setting> {
        super::settings::setting_rows(self)
            .get(self.settings_sel.peek())
            .map(|row| row.setting)
    }

    pub(in crate::tui::sigil_view) fn adjust_setting(&self) {
        use super::settings::Setting;

        if self.picker.open.peek() {
            return;
        }
        let Some(setting) = self.selected_setting() else {
            return;
        };
        match setting {
            Setting::Verify => self.toggle_verify(),
            Setting::Workflow => self.open_workflow(),
            Setting::Endpoint => self.cycle_endpoint(),
            Setting::Model => self.begin_model_edit(),
            Setting::Reasoning => self.reasoning_command(None),
            Setting::Tools => self.toggle_tools(),
            setting => self.adjust_display(setting),
        }
    }

    fn adjust_display(&self, setting: super::settings::Setting) {
        use super::settings::Setting;

        match setting {
            Setting::DiffLayout => {
                self.set_diff_layout(self.model.get().prefs.diff_layout.toggle());
            }
            Setting::Mail => self.toggle_mail(),
            Setting::Module(id) => self.toggle_module(id),
            Setting::Theme => self.toggle_theme(),
            Setting::Background => self.toggle_background(),
            Setting::Notify => self.flip(|prefs| prefs.notify = prefs.notify.toggle()),
            Setting::Receipts => self.receipts_command(None),
            Setting::Hotkeys => self.open_hotkeys(),
            Setting::Splash => self.flip(|prefs| prefs.splash = !prefs.splash),
            Setting::Motion => self.toggle_motion(),
            Setting::Mode => self.cycle_run_mode(),
            Setting::Verify
            | Setting::Workflow
            | Setting::Endpoint
            | Setting::Model
            | Setting::Reasoning
            | Setting::Tools => {}
        }
    }

    fn toggle_motion(&self) {
        self.flip(|prefs| prefs.animations = !prefs.animations);
        self.splash_since.set(std::time::Instant::now());
        self.splash_revealed.set(!self.model.get().prefs.animations);
    }

    pub(in crate::tui::sigil_view) fn flip(&self, change: impl FnOnce(&mut crate::prefs::Prefs)) {
        self.model_mut(|model| {
            change(&mut model.prefs);
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_options(&self) {
        match self.overlay() {
            Some(Overlay::Workflow) => self.workflow.open.set(false),
            Some(Overlay::Hotkeys) => self.close_hotkeys(),
            Some(Overlay::Settings) | None => {
                let open = !self.settings_open.peek();
                if open {
                    self.settings_sel.set(0);
                }
                self.settings_open.set(open);
            }
            Some(_) => {
                self.overlay_close();
            }
        }
    }

    pub(in crate::tui::sigil_view) fn open_hotkeys(&self) {
        self.hotkeys.sel.set(0);
        self.stop_recording();
        self.hotkeys.open.set(true);
    }

    pub(in crate::tui::sigil_view) fn close_hotkeys(&self) {
        self.stop_recording();
        self.hotkeys.open.set(false);
    }

    fn stop_recording(&self) {
        self.hotkeys.recording.set(false);
        self.hotkeys.armed.set(false);
        self.hotkeys.note.set(String::new());
    }

    fn save_hotkeys(&self) {
        let prefs = self.model.peek().prefs.clone();
        self.stop_recording();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn move_hotkey(&self, delta: isize) {
        if self.hotkeys.recording.peek() {
            return;
        }
        self.hotkeys.sel.set(
            self.hotkeys
                .sel
                .peek()
                .saturating_add_signed(delta)
                .min(Hotkey::ALL.len() - 1),
        );
    }

    pub(in crate::tui::sigil_view) fn edit_hotkey(&self) {
        if !self.hotkeys.recording.peek() {
            self.hotkeys.note.set(String::new());
            self.hotkeys.armed.set(false);
            self.hotkeys.recording.set(true);
        }
    }

    pub(in crate::tui::sigil_view) fn reset_hotkeys(&self) {
        self.model_mut(|model| {
            model.prefs.hotkeys = Default::default();
        });
        self.save_hotkeys();
    }

    pub(in crate::tui::sigil_view) fn arm_leader(&self) {
        self.hotkeys.note.set(String::new());
        self.hotkeys.armed.set(true);
    }

    pub(in crate::tui::sigil_view) fn take_chord(&self, key: char) {
        self.hotkeys.armed.set(false);
        if key == 'x' {
            return;
        }
        if self.hotkeys.recording.peek() {
            self.record_chord(key, true);
        } else {
            self.route_chord(key);
        }
    }

    pub(in crate::tui::sigil_view) fn route_hotkey(&self, key: char) -> bool {
        self.dispatch(self.model.get().prefs.hotkeys.action(key))
    }

    pub(in crate::tui::sigil_view) fn route_chord(&self, key: char) -> bool {
        self.hotkeys.armed.set(false);
        let found = self.model.get().prefs.hotkeys.chord(key);
        if found.is_none() {
            self.hotkeys.note.set(format!("ctrl+x {key} is unbound"));
            return true;
        }
        self.hotkeys.note.set(String::new());
        self.dispatch(found)
    }

    fn dispatch(&self, action: Option<Hotkey>) -> bool {
        let Some(action) = action else {
            return false;
        };
        if action != Hotkey::Options && self.options_active() {
            return true;
        }
        self.run_hotkey(action)
    }

    pub(in crate::tui::sigil_view) fn record_chord(&self, key: char, leader: bool) {
        let action = Hotkey::ALL[self.hotkeys.sel.peek().min(Hotkey::ALL.len() - 1)];
        let mut assigned = false;
        self.model_mut(|model| {
            assigned = model.prefs.hotkeys.assign(action, key, leader);
        });
        if !assigned {
            let binding = if leader {
                format!("ctrl+x {key}")
            } else {
                format!("ctrl+{key}")
            };
            self.hotkeys.note.set(format!("{binding} is reserved"));
            return;
        }
        self.save_hotkeys();
    }

    fn run_hotkey(&self, action: Hotkey) -> bool {
        match action {
            Hotkey::Options => self.toggle_options(),
            Hotkey::Newline => return false,
            Hotkey::Editor => self.editor_requested.set(true),
            Hotkey::Files => self.toggle_pane(Pane::Files),
            Hotkey::Ledger => self.toggle_todos(),
            Hotkey::Agents => self.toggle_agents(),
            Hotkey::Diff => self.toggle_pane(Pane::Diff),
            Hotkey::Copy => self.copy_chat(None),
            Hotkey::Select => self.select.set(!self.select.peek()),
            Hotkey::Pane => self.toggle_side(),
            Hotkey::Search => self.search.start(""),
            Hotkey::History => self.prompt_history(""),
            Hotkey::Fold => self.fold_cycle(),
        }
        true
    }

    pub(in crate::tui::sigil_view) fn toggle_theme(&self) {
        let names = crate::prefs::THEMES;
        self.model_mut(|model| {
            let current = crate::prefs::theme(&model.prefs.theme);
            let next = names
                .iter()
                .position(|name| *name == current)
                .map_or(names[0], |index| names[(index + 1) % names.len()]);
            model.prefs.theme = next.to_string();
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.apply_theme();
        self.persist(&prefs);
        self.refresh();
    }

    fn toggle_background(&self) {
        self.model_mut(|model| {
            model.prefs.background = model.prefs.background.toggle();
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.apply_theme();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn apply_theme(&self) {
        let prefs = self.model.peek().prefs.clone();
        for problem in theme::install(&prefs, &self.workspace, self.info.config.as_deref()) {
            self.push_entry(Entry::System(problem));
        }
    }

    pub(in crate::tui::sigil_view) fn toggle_agents(&self) {
        self.model_mut(|model| model.agents_expanded = !model.agents_expanded);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn clear_agent(&self, id: &str) {
        self.model_mut(|model| {
            if model
                .agents
                .iter()
                .any(|agent| agent.id == id && agent.background)
            {
                model.agents_hidden.insert(id.to_owned());
            }
        });
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_mail(&self) {
        self.model_mut(|model| {
            model.prefs.mail = !model.prefs.mail;
        });
        let prefs = self.model.peek().prefs.clone();
        let on = prefs.mail;
        self.persist(&prefs);
        drop(self.cmd.send(Cmd::SetMail(on)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_module(&self, id: crate::modules::ModuleId) {
        let mut modules = self.modules.peek();
        let on = !modules.get(&id).copied().unwrap_or(false);
        modules.insert(id, on);
        self.modules.set(modules);
        drop(self.cmd.send(Cmd::SetModule(id, on)));
        self.refresh();
    }
}
