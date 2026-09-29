use super::*;

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
            Setting::Mode => {
                if self.model.get().snapshot.plan {
                    self.leave_plan();
                } else if self.model.get().prefs.plan_mode {
                    self.enter_plan();
                }
            }
            Setting::PlanMode => self.flip(|prefs| prefs.plan_mode = !prefs.plan_mode),
            Setting::Verify => self.toggle_verify(),
            Setting::Workflow => self.open_workflow(),
            Setting::Endpoint => self.cycle_endpoint(),
            Setting::Model => self.begin_model_edit(),
            Setting::Reasoning => self.reasoning_command(None),
            Setting::Tools => self.toggle_tools(),
            Setting::DiffLayout => {
                let layout = self.model.get().prefs.diff_layout.toggle();
                self.set_diff_layout(layout);
            }
            Setting::Mail => self.toggle_mail(),
            Setting::Module(index) => {
                if let Some(module) = crate::modules::ALL.get(index) {
                    self.toggle_module(module.id.name());
                }
            }
            Setting::Theme => self.toggle_theme(),
            Setting::Receipts => self.receipts_command(None),
            Setting::Hotkeys => self.open_hotkeys(),
            Setting::Splash => self.flip(|prefs| prefs.splash = !prefs.splash),
            Setting::Motion => {
                self.flip(|prefs| prefs.animations = !prefs.animations);
                self.splash_since.set(std::time::Instant::now());
                self.splash_revealed.set(!self.model.get().prefs.animations);
            }
        }
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
        self.hotkeys.recording.set(false);
        self.hotkeys.note.set(String::new());
        self.hotkeys.open.set(true);
    }

    pub(in crate::tui::sigil_view) fn close_hotkeys(&self) {
        self.hotkeys.recording.set(false);
        self.hotkeys.note.set(String::new());
        self.hotkeys.open.set(false);
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
            self.hotkeys.recording.set(true);
        }
    }

    pub(in crate::tui::sigil_view) fn reset_hotkeys(&self) {
        self.model_mut(|model| {
            model.prefs.hotkeys = Default::default();
        });
        let prefs = self.model.peek().prefs.clone();
        self.hotkeys.recording.set(false);
        self.hotkeys.note.set(String::new());
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn route_hotkey(&self, key: char) -> bool {
        if self.hotkeys.recording.peek() {
            if !crate::prefs::available(key) {
                self.hotkeys.note.set(format!("ctrl+{key} is reserved"));
                return true;
            }
            let action = Hotkey::ALL[self.hotkeys.sel.peek().min(Hotkey::ALL.len() - 1)];
            self.model_mut(|model| {
                let _ = model.prefs.hotkeys.assign(action, key);
            });
            let prefs = self.model.peek().prefs.clone();
            self.hotkeys.recording.set(false);
            self.hotkeys.note.set(String::new());
            self.persist(&prefs);
            self.refresh();
            return true;
        }
        let action = self.model.get().prefs.hotkeys.action(key);
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
            Hotkey::Agents => self.toggle_agents(),
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
        self.model_mut(|model| {
            let next = names
                .iter()
                .position(|(name, _)| *name == model.prefs.theme)
                .map_or(names[0].0, |index| names[(index + 1) % names.len()].0);
            model.prefs.theme = next.to_string();
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        theme::install(&prefs.theme, &self.workspace, self.info.config.as_deref());
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn hide_agents(&self) {
        self.model_mut(|model| model.agents_hidden = true);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_agents(&self) {
        self.model_mut(|model| model.agents_expanded = !model.agents_expanded);
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

    pub(in crate::tui::sigil_view) fn toggle_module(&self, name: &'static str) {
        let mut modules = self.modules.peek();
        let on = !modules.get(name).copied().unwrap_or(false);
        modules.insert(name, on);
        self.modules.set(modules);
        drop(self.cmd.send(Cmd::SetModule(name.to_string(), on)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn commit_effort(&self) {
        let level = self.effort.picked();
        self.effort.open.set(false);
        self.effort_command(level.as_str());
    }

    pub(in crate::tui::sigil_view) fn effort_command(&self, level: &str) {
        let next = match crate::provider::Effort::parse(level) {
            Some(effort) => effort,
            None => {
                self.push_entry(Entry::Error(format!(
                    "effort {}",
                    crate::provider::Effort::rejects(level)
                )));
                self.refresh();
                return;
            }
        };
        self.model_mut(|model| model.snapshot.effort = Some(next));
        drop(self.cmd.send(Cmd::SetEffort(Some(next))));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn reasoning_command(&self, argument: Option<&str>) {
        self.model_mut(|model| {
            model.prefs.reasoning = match argument {
                Some(value) => {
                    crate::prefs::Visibility::parse(value, crate::prefs::Visibility::Collapsed)
                }
                None => model.prefs.reasoning.toggle(),
            };
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn receipts_command(&self, argument: Option<&str>) {
        self.model_mut(|model| {
            model.prefs.receipts = match argument {
                Some(value) => {
                    crate::prefs::Visibility::parse(value, crate::prefs::Visibility::Off)
                }
                None => model.prefs.receipts.toggle(),
            };
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_tools(&self) {
        self.model_mut(|model| {
            model.prefs.tools = model.prefs.tools.toggle();
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }
}
