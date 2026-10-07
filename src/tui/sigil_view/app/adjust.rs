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
            Setting::Mode => self.toggle_mode(),
            Setting::PlanMode => self.flip(|prefs| prefs.plan_mode = !prefs.plan_mode),
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
            Setting::Mode
            | Setting::PlanMode
            | Setting::Verify
            | Setting::Workflow
            | Setting::Endpoint
            | Setting::Model
            | Setting::Reasoning
            | Setting::Tools => {}
        }
    }

    fn toggle_mode(&self) {
        if self.model.get().snapshot.plan {
            self.leave_plan();
        } else if self.model.get().prefs.plan_mode {
            self.enter_plan();
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
            self.record_hotkey(key);
            return true;
        }
        let action = self.model.get().prefs.hotkeys.action(key);
        let Some(action) = action else {
            return false;
        };
        if action != Hotkey::Options && self.options_active() {
            return true;
        }
        self.run_hotkey(action)
    }

    fn record_hotkey(&self, key: char) {
        let action = Hotkey::ALL[self.hotkeys.sel.peek().min(Hotkey::ALL.len() - 1)];
        let mut assigned = false;
        if crate::prefs::available(key) {
            self.model_mut(|model| {
                assigned = model.prefs.hotkeys.assign(action, key);
            });
        }
        if !assigned {
            self.hotkeys.note.set(format!("ctrl+{key} is reserved"));
            return;
        }
        let prefs = self.model.peek().prefs.clone();
        self.hotkeys.recording.set(false);
        self.hotkeys.note.set(String::new());
        self.persist(&prefs);
        self.refresh();
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

    pub(in crate::tui::sigil_view) fn commit_effort(&self) {
        if self.model.peek().snapshot.levels.is_empty() {
            self.effort.open.set(false);
            self.refresh();
            return;
        }
        let level = self.effort.picked();
        self.effort.open.set(false);
        self.effort_command(level.as_str());
    }

    pub(in crate::tui::sigil_view) fn effort_command(&self, level: &str) {
        let next = match crate::provider::Effort::parse(level) {
            Some(effort) => effort.clamp(&self.model.peek().snapshot.levels),
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
        self.set_tools(self.model.peek().prefs.tools.toggle());
    }

    pub(in crate::tui::sigil_view) fn set_tools(&self, tools: crate::prefs::ToolOutput) {
        self.model_mut(|model| {
            model.prefs.tools = tools;
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }

    fn fold_cycle(&self) {
        use crate::prefs::{ToolOutput, Visibility};
        let prefs = &self.model.peek().prefs;
        let (reasoning, tools) = match (prefs.reasoning, prefs.tools) {
            (_, ToolOutput::Hidden) => (Visibility::Expanded, ToolOutput::Compact),
            (Visibility::Expanded, _) => (Visibility::Collapsed, ToolOutput::Compact),
            _ => (Visibility::Collapsed, ToolOutput::Hidden),
        };
        self.model_mut(|model| {
            model.prefs.reasoning = reasoning;
            model.prefs.tools = tools;
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }
}
