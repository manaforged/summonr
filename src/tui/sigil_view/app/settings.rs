use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn move_setting(&self, delta: isize) {
        if self.editing_model.peek() {
            return;
        }
        let last = super::super::settings::setting_rows(self)
            .len()
            .saturating_sub(1);
        self.settings_sel.set(
            self.settings_sel
                .peek()
                .saturating_add_signed(delta)
                .min(last),
        );
    }

    pub(in crate::tui::sigil_view) fn selected_setting(
        &self,
    ) -> Option<super::super::settings::Setting> {
        super::super::settings::setting_rows(self)
            .get(self.settings_sel.peek())
            .map(|row| row.setting)
    }

    pub(in crate::tui::sigil_view) fn adjust_setting(&self) {
        use super::super::settings::Setting;

        if self.editing_model.peek() {
            return;
        }
        let Some(setting) = self.selected_setting() else {
            return;
        };
        match setting {
            Setting::Mode => self.toggle_mode(),
            Setting::Verify => self.toggle_verify(),
            Setting::Workflow => self.open_workflow(),
            Setting::Endpoint => self.cycle_endpoint(),
            Setting::Model => self.begin_model_edit(),
            Setting::Reasoning => self.reasoning_command(None),
            Setting::Tools => self.toggle_tools(),
            Setting::DiffLayout => {
                let layout = self.model.borrow().prefs.diff_layout.toggle();
                self.set_diff_layout(layout);
            }
            Setting::Bus => self.toggle_bus(),
            Setting::Module(index) => {
                if let Some(module) = crate::modules::ALL.get(index) {
                    self.toggle_module(module.name);
                }
            }
            Setting::Theme => self.toggle_theme(),
            Setting::Receipts => self.receipts_command(None),
            Setting::Hotkeys => self.open_hotkeys(),
            Setting::Comments => self.flip(|prefs| prefs.ban_comments = !prefs.ban_comments),
            Setting::Tests => self.flip(|prefs| prefs.ban_tests = !prefs.ban_tests),
            Setting::Splash => self.flip(|prefs| prefs.splash = !prefs.splash),
            Setting::Motion => {
                self.flip(|prefs| prefs.animations = !prefs.animations);
                self.splash_revealed
                    .set(!self.model.borrow().prefs.animations);
            }
        }
    }

    pub(in crate::tui::sigil_view) fn flip(&self, change: impl FnOnce(&mut crate::prefs::Prefs)) {
        let mut model = self.model.borrow_mut();
        change(&mut model.prefs);
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_options(&self) {
        match self.overlay() {
            Some(Overlay::Workflow) => self.workflow_open.set(false),
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
        self.hotkey_sel.set(0);
        self.hotkey_recording.set(false);
        self.hotkey_note.set(String::new());
        self.hotkeys_open.set(true);
    }

    pub(in crate::tui::sigil_view) fn close_hotkeys(&self) {
        self.hotkey_recording.set(false);
        self.hotkey_note.set(String::new());
        self.hotkeys_open.set(false);
    }

    pub(in crate::tui::sigil_view) fn move_hotkey(&self, delta: isize) {
        if self.hotkey_recording.peek() {
            return;
        }
        self.hotkey_sel.set(
            self.hotkey_sel
                .peek()
                .saturating_add_signed(delta)
                .min(Hotkey::ALL.len() - 1),
        );
    }

    pub(in crate::tui::sigil_view) fn edit_hotkey(&self) {
        if !self.hotkey_recording.peek() {
            self.hotkey_note.set(String::new());
            self.hotkey_recording.set(true);
        }
    }

    pub(in crate::tui::sigil_view) fn reset_hotkeys(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.hotkeys = Default::default();
        let prefs = model.prefs.clone();
        drop(model);
        self.hotkey_recording.set(false);
        self.hotkey_note.set(String::new());
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn route_hotkey(&self, key: char) -> bool {
        if self.hotkey_recording.peek() {
            if !crate::prefs::available(key) {
                self.hotkey_note.set(format!("ctrl+{key} is reserved"));
                return true;
            }
            let action = Hotkey::ALL[self.hotkey_sel.peek().min(Hotkey::ALL.len() - 1)];
            let mut model = self.model.borrow_mut();
            let _ = model.prefs.hotkeys.assign(action, key);
            let prefs = model.prefs.clone();
            drop(model);
            self.hotkey_recording.set(false);
            self.hotkey_note.set(String::new());
            self.persist(&prefs);
            self.refresh();
            return true;
        }
        let action = self.model.borrow().prefs.hotkeys.action(key);
        let Some(action) = action else {
            return false;
        };
        if action != Hotkey::Options && self.options_active() {
            return true;
        }
        match action {
            Hotkey::Options => self.toggle_options(),
            Hotkey::Newline => return false,
            Hotkey::Editor => self.editor_requested.set(true),
            Hotkey::Files => self.toggle_pane(Pane::Files),
            Hotkey::Ledger => self.toggle_todos(),
            Hotkey::Diff => self.toggle_pane(Pane::Diff),
            Hotkey::Copy => self.copy_chat(None),
            Hotkey::Select => self.select.set(!self.select.peek()),
            Hotkey::Pane => self.toggle_side(),
            Hotkey::Search => self.search.start(""),
            Hotkey::History => self.prompt_history(""),
        }
        true
    }

    pub(in crate::tui::sigil_view) fn toggle_theme(&self) {
        let names = theme::Palette::ALL;
        let mut model = self.model.borrow_mut();
        let next = names
            .iter()
            .position(|(name, _)| *name == model.prefs.theme)
            .map_or(names[0].0, |index| names[(index + 1) % names.len()].0);
        model.prefs.theme = next.to_string();
        model.entries.invalidate();
        let prefs = model.prefs.clone();
        drop(model);
        theme::install(&prefs.theme, &self.workspace, self.info.config.as_deref());
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn hide_agents(&self) {
        self.model.borrow_mut().agents_hidden = true;
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_bus(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.bus = !model.prefs.bus;
        let on = model.prefs.bus;
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        drop(self.cmd.send(Cmd::SetBus(on)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_module(&self, name: &'static str) {
        let mut modules = self.modules.peek();
        let on = !modules.get(name).copied().unwrap_or(false);
        modules.insert(name, on);
        self.modules.set(modules);
        drop(self.cmd.send(Cmd::SetModule(name.to_string(), on)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn effort_command(&self, argument: Option<&str>) {
        let next = match argument {
            None => self
                .model
                .borrow()
                .snapshot
                .effort
                .unwrap_or(crate::provider::Effort::None)
                .next(),
            Some(level) => match crate::provider::Effort::parse(level) {
                Some(effort) => effort,
                None => {
                    self.model.borrow_mut().entries.push(Entry::Error(format!(
                        "effort takes none, low, medium, or high, not `{level}`"
                    )));
                    self.refresh();
                    return;
                }
            },
        };
        self.model.borrow_mut().snapshot.effort = Some(next);
        drop(self.cmd.send(Cmd::SetEffort(Some(next))));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn reasoning_command(&self, argument: Option<&str>) {
        let mut model = self.model.borrow_mut();
        model.prefs.reasoning = match argument {
            Some(value) => {
                crate::prefs::Visibility::parse(value, crate::prefs::Visibility::Collapsed)
            }
            None => model.prefs.reasoning.toggle(),
        };
        model.entries.invalidate();
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn receipts_command(&self, argument: Option<&str>) {
        let mut model = self.model.borrow_mut();
        model.prefs.receipts = match argument {
            Some(value) => crate::prefs::Visibility::parse(value, crate::prefs::Visibility::Off),
            None => model.prefs.receipts.toggle(),
        };
        model.entries.invalidate();
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_tools(&self) {
        let mut model = self.model.borrow_mut();
        model.prefs.tools = model.prefs.tools.toggle();
        model.entries.invalidate();
        let prefs = model.prefs.clone();
        drop(model);
        self.persist(&prefs);
        self.refresh();
    }
}
