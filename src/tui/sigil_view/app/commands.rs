use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn suggesting(&self) -> bool {
        !self.settings_open.peek()
            && !self.workflow_open.peek()
            && !self.editing_model.peek()
            && self.model.borrow().busy_since.is_none()
            && self
                .commands
                .open(&self.draft.peek(), self.caret.peek(), &self.model.borrow())
    }

    pub(in crate::tui::sigil_view) fn completion(&self, value: &str) {
        let mut draft = self.draft.peek();
        let caret = self.caret.peek();
        let Some((start, _)) = super::super::suggest::mention(&draft, caret) else {
            self.editor.replace(value);
            self.select(value);
            return;
        };
        let end = if draft[start + 1..].starts_with('"') {
            let mut parsed =
                serde_json::Deserializer::from_str(&draft[start + 1..]).into_iter::<String>();
            if matches!(parsed.next(), Some(Ok(_))) {
                start + 1 + parsed.byte_offset()
            } else {
                caret
            }
        } else {
            caret
                + draft[caret..]
                    .find(char::is_whitespace)
                    .unwrap_or(draft.len() - caret)
        };
        let following = draft[end..].chars().next().filter(|ch| ch.is_whitespace());
        let peer = self
            .model
            .borrow()
            .peers
            .iter()
            .any(|agent| agent.id == value);
        let mut reference = if peer {
            format!("@{value}")
        } else {
            super::super::suggest::reference(value)
        };
        if following.is_none() {
            reference.push(' ');
        }
        draft.replace_range(start..end, &reference);
        self.edit(&draft);
        self.caret
            .set(start + reference.len() + following.map_or(0, char::len_utf8));
    }

    pub(in crate::tui::sigil_view) fn local(&self, line: &str, argument: Option<&str>) -> bool {
        match crate::parse_command(line) {
            Some(crate::SlashCommand::Help) => self.help.show(),
            Some(crate::SlashCommand::Retry) => self.retry(),
            Some(crate::SlashCommand::Search) => self.search.start(argument.unwrap_or("")),
            Some(crate::SlashCommand::History) => self.prompt_history(argument.unwrap_or("")),
            Some(crate::SlashCommand::Branch) => {
                if self.model.borrow().busy_since.is_some() {
                    self.model.borrow_mut().entries.push(Entry::Error(
                        "Wait for the active operation or cancel it before branching.".to_string(),
                    ));
                    self.refresh();
                } else {
                    self.search.branch(argument.unwrap_or(""));
                    drop(
                        self.cmd
                            .send(Cmd::ListPrompts(Some(self.model.borrow().session.clone()))),
                    );
                }
            }
            Some(crate::SlashCommand::Resume | crate::SlashCommand::Tree) if argument.is_none() => {
                if self.model.borrow().busy_since.is_some() {
                    self.model.borrow_mut().entries.push(Entry::Error(
                        "Cancel the active turn before resuming a session.".to_string(),
                    ));
                    self.refresh();
                } else {
                    self.sessions.start(
                        &self.model.borrow().session,
                        crate::parse_command(line) == Some(crate::SlashCommand::Tree),
                    );
                    drop(self.cmd.send(Cmd::ListSessions));
                }
            }
            Some(crate::SlashCommand::Usage) => {
                self.usage.start();
                drop(self.cmd.send(Cmd::ListUsage));
            }
            Some(crate::SlashCommand::Settings) => {
                self.hotkeys_open.set(false);
                self.settings_open.set(true);
            }
            Some(crate::SlashCommand::Auth | crate::SlashCommand::Login) => {
                self.begin_auth(argument);
            }
            Some(crate::SlashCommand::Model) => {
                let model = argument.unwrap_or_default();
                match model {
                    "" => self.begin_model_edit(),
                    "next" => self.cycle_seat(true),
                    "prev" | "previous" => self.cycle_seat(false),
                    model => drop(self.cmd.send(Cmd::SetModel(model.to_string()))),
                }
            }
            Some(crate::SlashCommand::Workflow) if argument.is_none() => {
                self.open_workflow();
            }
            Some(crate::SlashCommand::Effort) => self.effort_command(argument),
            Some(crate::SlashCommand::Stats) => {
                self.toggle_meter(super::super::hit::Target::Context)
            }
            Some(crate::SlashCommand::Theme) => self.toggle_theme(),
            Some(crate::SlashCommand::Tools) => self.toggle_tools(),
            Some(crate::SlashCommand::Reasoning) => self.reasoning_command(argument),
            Some(crate::SlashCommand::Receipts) => self.receipts_command(argument),
            Some(crate::SlashCommand::Pane) => self.pane_command(argument),
            Some(crate::SlashCommand::Layout) => self.layout_command(argument),
            Some(crate::SlashCommand::Diff) => self.diff_command(argument),
            Some(crate::SlashCommand::Copy) => self.copy_chat(argument),
            Some(crate::SlashCommand::Export) => {
                drop(self.cmd.send(Cmd::Export(argument.map(str::to_string))));
            }
            Some(crate::SlashCommand::View) => self.view_command(argument),
            Some(crate::SlashCommand::Bus) => self.toggle_bus(),
            _ => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn toggle_mode(&self) {
        let mode = {
            let snapshot = &self.model.borrow().snapshot;
            if snapshot.plan {
                crate::RunMode::Plan
            } else if snapshot.work {
                crate::RunMode::Work
            } else if snapshot.goal {
                crate::RunMode::Goal
            } else {
                crate::RunMode::Auto
            }
        };
        let next = mode.next();
        let mut model = self.model.borrow_mut();
        model.snapshot.plan = next == crate::RunMode::Plan;
        model.snapshot.work = next == crate::RunMode::Work;
        model.snapshot.goal = next == crate::RunMode::Goal;
        drop(model);
        drop(self.cmd.send(Cmd::SetRunMode(next)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_verify(&self) {
        let on = self.model.borrow().snapshot.verify.is_none();
        drop(self.cmd.send(Cmd::SetVerify(on)));
    }

    pub(in crate::tui::sigil_view) fn cycle_endpoint(&self) {
        self.cycle_seat(true);
    }

    pub(in crate::tui::sigil_view) fn cycle_seat(&self, forward: bool) {
        let current = self.model.borrow().snapshot.endpoint.clone();
        match crate::endpoint::step(&current, forward) {
            Some(next) => drop(self.cmd.send(Cmd::SetEndpoint(next))),
            None => {
                self.model.borrow_mut().entries.push(Entry::System(
                    "only one ready seat; `summon login NAME` adds another".to_string(),
                ));
                self.refresh();
            }
        }
    }
}
