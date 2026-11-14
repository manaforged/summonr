use sigil::widgets::CursorEdge;

use super::settings::{Group, Setting};
use super::*;

const AUTO_COMMAND: &str = "/auto";
const GOAL_PREFILL: &str = "/goal ";

mod display;

impl App {
    pub(crate) fn move_setting(&self, delta: isize) {
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

    pub(crate) fn selected_setting(&self) -> Option<Setting> {
        let group = self.settings_group.peek()?;
        Setting::all()
            .filter(|setting| setting.group() == group)
            .nth(self.settings.cursor.peek())
    }

    pub(crate) fn adjust_setting(&self) {
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

    pub(crate) fn close_settings(&self) {
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

    pub(crate) fn cycle_splash(&self) {
        self.flip(|prefs| prefs.splash = prefs.splash.next());
        let model = self.model.peek();
        self.splash
            .set(model.prefs.splash.shown() && model.entries.is_empty());
        self.splash_since.set(std::time::Instant::now());
        self.splash_revealed.set(!model.prefs.animations);
    }

    pub(crate) fn toggle_motion(&self) {
        self.flip(|prefs| prefs.animations = !prefs.animations);
        self.splash_since.set(std::time::Instant::now());
        self.splash_revealed.set(!self.model.get().prefs.animations);
    }

    pub(crate) fn flip(&self, change: impl FnOnce(&mut summonr_agent::prefs::Prefs)) {
        self.model_mut(|model| {
            change(&mut model.prefs);
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }

    pub(crate) fn toggle_options(&self) {
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

    pub(crate) fn open_hotkeys(&self) {
        self.hotkeys.list.cursor.set(0);
        self.stop_recording();
        self.hotkeys.open.set(true);
    }

    pub(crate) fn close_hotkeys(&self) {
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

    pub(crate) fn move_hotkey(&self, delta: isize) {
        if self.hotkeys.recording.peek() {
            return;
        }
        self.hotkeys
            .list
            .shift(delta, Hotkey::ALL.len(), CursorEdge::Clamp);
    }

    pub(crate) fn edit_hotkey(&self) {
        if !self.hotkeys.recording.peek() {
            self.hotkeys.note.set(String::new());
            self.hotkeys.armed.set(false);
            self.hotkeys.recording.set(true);
        }
    }

    pub(crate) fn reset_hotkeys(&self) {
        self.model_mut(|model| {
            model.prefs.hotkeys = Default::default();
        });
        self.save_hotkeys();
    }

    pub(crate) fn arm_leader(&self) {
        self.hotkeys.note.set(String::new());
        self.hotkeys.armed.set(true);
    }

    pub(crate) fn take_chord(&self, key: char) {
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

    pub(crate) fn route_hotkey(&self, key: char) -> bool {
        self.dispatch(self.model.get().prefs.hotkeys.action(key))
    }

    pub(crate) fn route_chord(&self, key: char) -> bool {
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

    pub(crate) fn record_chord(&self, key: char, leader: bool) {
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

    pub(crate) fn toggle_background(&self) {
        self.model_mut(|model| {
            model.prefs.background = model.prefs.background.toggle();
            model.entries.invalidate();
        });
        let prefs = self.model.peek().prefs.clone();
        self.apply_theme();
        self.persist(&prefs);
        self.refresh();
    }

    pub(crate) fn follow_backdrop(&self) {
        let app = self.clone();
        let seen = std::cell::Cell::new(None);
        sigil::prelude::effect(move || {
            let backdrop = sigil::terminal::backdrop().get();
            let changed = seen
                .replace(Some(backdrop))
                .is_some_and(|before| before != backdrop);
            if changed && app.model.peek().prefs.theme == summonr_agent::prefs::AUTO_THEME {
                app.model_mut(|model| model.entries.invalidate());
                app.apply_theme();
                app.refresh();
            }
        });
    }

    pub(crate) fn apply_theme(&self) {
        let prefs = self.model.peek().prefs.clone();
        let problems = theme::install(&prefs, &self.workspace(), self.info.config.as_deref());
        self.appearance.set(theme::palette());
        for problem in problems {
            self.push_entry(Entry::System(problem));
        }
    }

    pub(crate) fn clear_agent(&self, id: &str) {
        self.model_mut(|model| model.clear_agent(id));
        self.refresh();
    }

    pub(crate) fn toggle_mail(&self) {
        self.model_mut(|model| {
            model.prefs.mail = !model.prefs.mail;
        });
        let prefs = self.model.peek().prefs.clone();
        let on = prefs.mail;
        self.persist(&prefs);
        drop(self.cmd.send(Cmd::SetMail(on)));
        self.refresh();
    }

    /// Asks for the Chrome-family browser the browser tools drive, and saves the choice.
    pub(crate) fn choose_chrome(&self) {
        self.window.choose_chrome(&self.cmd);
    }

    pub(crate) fn toggle_module(&self, id: summonr_agent::modules::ModuleId) {
        let mut modules = self.modules.peek();
        let on = !modules.get(&id).copied().unwrap_or(false);
        modules.insert(id, on);
        self.modules.set(modules);
        drop(self.cmd.send(Cmd::SetModule(id, on)));
        self.refresh();
    }
}
