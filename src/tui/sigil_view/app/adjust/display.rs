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

    pub(in crate::tui::sigil_view) fn set_visibility(
        &self,
        field: fn(&mut crate::prefs::Prefs) -> &mut crate::prefs::Visibility,
        next: Option<crate::prefs::Visibility>,
    ) {
        self.flip(|prefs| {
            let shown = field(prefs);
            *shown = next.unwrap_or_else(|| shown.toggle());
        });
    }

    pub(in crate::tui::sigil_view) fn toggle_tools(&self) {
        self.set_tools(self.model.peek().prefs.tools.toggle());
    }

    pub(in crate::tui::sigil_view) fn set_tools(&self, tools: crate::prefs::ToolOutput) {
        self.flip(|prefs| prefs.tools = tools);
    }

    pub(in crate::tui::sigil_view) fn fold_cycle(&self) {
        use crate::prefs::{ToolOutput, Visibility};
        let prefs = &self.model.peek().prefs;
        let (reasoning, tools) = match (prefs.reasoning, prefs.tools) {
            (_, ToolOutput::Hidden) => (Visibility::Expanded, ToolOutput::Compact),
            (Visibility::Expanded, _) => (Visibility::Collapsed, ToolOutput::Compact),
            _ => (Visibility::Collapsed, ToolOutput::Hidden),
        };
        self.flip(|prefs| {
            prefs.reasoning = reasoning;
            prefs.tools = tools;
        });
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
