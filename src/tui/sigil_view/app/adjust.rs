use sigil::widgets::CursorEdge;

use super::settings::{Group, Setting};
use super::*;

const AUTO_COMMAND: &str = "/auto";
const GOAL_PREFILL: &str = "/goal ";

mod display;

impl App {
    pub(in crate::tui::sigil_view) fn move_setting(&self, delta: isize) {
        if self.picker.open.peek() {
            return;
        }
        let count = self
            .settings_group
            .peek()
            .map_or(Group::ALL.len(), |group| {
                Setting::all()
                    .filter(|setting| setting.group() == group)
                    .count()
            });
        self.settings.shift(delta, count, CursorEdge::Clamp);
    }

    pub(in crate::tui::sigil_view) fn selected_setting(&self) -> Option<Setting> {
        let group = self.settings_group.peek()?;
        Setting::all()
            .filter(|setting| setting.group() == group)
            .nth(self.settings.cursor.peek())
    }

    pub(in crate::tui::sigil_view) fn adjust_setting(&self) {
        if self.picker.open.peek() {
            return;
        }
        if self.settings_group.peek().is_none() {
            if let Some(group) = Group::ALL.get(self.settings.cursor.peek()) {
                self.settings_group.set(Some(*group));
                self.settings.select(0);
            }
        } else if let Some(setting) = self.selected_setting() {
            setting.activate(self);
        }
    }

    pub(in crate::tui::sigil_view) fn close_settings(&self) {
        if let Some(group) = self.settings_group.peek() {
            self.settings_group.set(None);
            self.settings.select(
                Group::ALL
                    .iter()
                    .position(|item| *item == group)
                    .unwrap_or(0),
            );
        } else {
            self.settings_open.set(false);
        }
    }

    pub(in crate::tui::sigil_view) fn toggle_motion(&self) {
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
                    self.settings.select(0);
                    self.settings_group.set(None);
                }
                self.settings_open.set(open);
            }
            Some(_) => {
                self.overlay_close();
            }
        }
    }

    pub(in crate::tui::sigil_view) fn open_hotkeys(&self) {
        self.hotkeys.list.cursor.set(0);
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
        self.hotkeys
            .list
            .shift(delta, Hotkey::ALL.len(), CursorEdge::Clamp);
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
        let status_key = self.overlay() == Some(Overlay::Status)
            && matches!(action, Hotkey::Agents | Hotkey::Ledger);
        if action != Hotkey::Options && self.options_active() && !status_key {
            return true;
        }
        self.run_key(super::super::view::KeyCommand::Hotkey(action))
    }

    pub(in crate::tui::sigil_view) fn record_chord(&self, key: char, leader: bool) {
        let action = Hotkey::ALL[self.hotkeys.list.at(Hotkey::ALL.len())];
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

    pub(in crate::tui::sigil_view) fn toggle_background(&self) {
        self.model_mut(|model| {
            model.prefs.background = model.prefs.background.toggle();
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.apply_theme();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn follow_backdrop(&self) {
        let app = self.clone();
        let seen = std::cell::Cell::new(None);
        sigil::prelude::effect(move || {
            let backdrop = sigil::terminal::backdrop().get();
            let changed = seen
                .replace(Some(backdrop))
                .is_some_and(|before| before != backdrop);
            if changed && app.model.peek().prefs.theme == crate::prefs::AUTO_THEME {
                app.model_mut(|model| model.entries.invalidate());
                app.apply_theme();
                app.refresh();
            }
        });
    }

    pub(in crate::tui::sigil_view) fn apply_theme(&self) {
        let prefs = self.model.peek().prefs.clone();
        let problems = theme::install(&prefs, &self.workspace, self.info.config.as_deref());
        self.appearance.set(theme::palette());
        for problem in problems {
            self.push_entry(Entry::System(problem));
        }
    }

    pub(in crate::tui::sigil_view) fn clear_agent(&self, id: &str) {
        self.model_mut(|model| model.clear_agent(id));
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
