use super::*;

impl App {
    pub(crate) fn commit_effort(&self) {
        if self.model.peek().snapshot.levels.is_empty() {
            self.effort.open.set(false);
            self.refresh();
            return;
        }
        let level = self.effort.picked();
        self.effort.open.set(false);
        self.effort_command(level.as_str());
    }

    pub(crate) fn effort_command(&self, level: &str) {
        let next = match summonr_agent::provider::Effort::parse(level) {
            Some(effort) => effort.clamp(&self.model.peek().snapshot.levels),
            None => {
                self.push_entry(Entry::Error(format!(
                    "effort {}",
                    summonr_agent::provider::Effort::rejects(level)
                )));
                self.refresh();
                return;
            }
        };
        self.set_effort(next);
    }

    pub(in crate::app) fn set_effort(&self, next: summonr_agent::provider::Effort) {
        self.model_mut(|model| std::sync::Arc::make_mut(&mut model.snapshot).effort = Some(next));
        drop(self.cmd.send(Cmd::SetEffort(Some(next))));
        self.refresh();
    }

    pub(crate) fn set_visibility(
        &self,
        field: fn(&mut summonr_agent::prefs::Prefs) -> &mut summonr_agent::prefs::Visibility,
        next: Option<summonr_agent::prefs::Visibility>,
    ) {
        self.flip(|prefs| {
            let shown = field(prefs);
            *shown = next.unwrap_or_else(|| shown.toggle());
        });
    }

    pub(crate) fn toggle_tools(&self) {
        self.set_tools(self.model.peek().prefs.tools.toggle());
    }

    pub(crate) fn set_tools(&self, tools: summonr_agent::prefs::ToolOutput) {
        self.flip(|prefs| prefs.tools = tools);
    }

    pub(crate) fn fold_cycle(&self) {
        use summonr_agent::prefs::{ToolOutput, Visibility};
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

    pub(crate) fn cycle_run_mode(&self) {
        if self.model.peek().snapshot.mode != summonr_agent::WorkKind::Single {
            drop(self.cmd.send(Cmd::Command(AUTO_COMMAND.to_string())));
        } else {
            self.settings_open.set(false);
            self.replace(GOAL_PREFILL.to_string());
        }
        self.refresh();
    }
}
