use super::*;

impl App {
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

    pub(super) fn fold_cycle(&self) {
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

    pub(super) fn cycle_run_mode(&self) {
        let (goal, work) = {
            let model = self.model.peek();
            (model.snapshot.goal, model.snapshot.work)
        };
        if goal || work {
            drop(self.cmd.send(Cmd::Command(AUTO_COMMAND.to_string())));
        } else {
            self.settings_open.set(false);
            self.replace(GOAL_PREFILL.to_string());
        }
        self.refresh();
    }
}
