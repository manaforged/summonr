use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn suggesting(&self) -> bool {
        !self.settings_open.peek()
            && !self.workflow.open.peek()
            && !self.picker.open.peek()
            && self.model.get().busy_since.is_none()
            && self
                .commands
                .open(&self.draft.peek(), self.caret.peek(), &self.model.get())
    }

    pub(in crate::tui::sigil_view) fn completion(&self, value: &str) {
        let mut draft = self.draft.peek();
        let caret = self.caret.peek();
        let Some((start, _)) = super::suggest::mention(&draft, caret) else {
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
        let peer = self.model.get().peers.iter().any(|agent| agent.id == value);
        let mut reference = if peer {
            format!("@{value}")
        } else {
            super::suggest::reference(value)
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
            Some(crate::SlashCommand::Fork) if argument.is_some() => {
                if self.model.get().busy_since.is_some() {
                    self.push_entry(Entry::Error(
                        "Wait for the active operation or cancel it before forking.".to_string(),
                    ));
                    self.refresh();
                } else {
                    self.search.branch(argument.unwrap_or(""));
                    drop(
                        self.cmd
                            .send(Cmd::ListPrompts(Some(self.model.get().session.clone()))),
                    );
                }
            }
            Some(crate::SlashCommand::Resume) if argument.is_none() => self.open_sessions(false),
            Some(crate::SlashCommand::Usage) if argument.is_none() => {
                self.usage.start();
                drop(self.cmd.send(Cmd::ListUsage));
            }
            Some(crate::SlashCommand::Settings) => {
                self.hotkeys.open.set(false);
                self.settings_open.set(true);
            }
            Some(crate::SlashCommand::Login) => self.begin_auth(argument),
            Some(crate::SlashCommand::Model) => self.model_command(argument),
            Some(crate::SlashCommand::Effort) => match argument {
                Some(level) => self.effort_command(level),
                None => self.effort.show(self.model.peek().snapshot.effort),
            },
            Some(crate::SlashCommand::Workflow) if argument.is_none() => {
                self.open_workflow();
            }
            Some(crate::SlashCommand::Pane) => self.pane_command(argument),
            Some(crate::SlashCommand::Diff) => self.diff_command(argument),
            Some(crate::SlashCommand::Copy) => self.copy_chat(argument),
            Some(crate::SlashCommand::Export) => {
                drop(self.cmd.send(Cmd::Export(argument.map(str::to_string))));
            }
            Some(crate::SlashCommand::View) => self.view_command(argument),
            Some(crate::SlashCommand::Demo) => {
                self.model_mut(|model| model.entries.extend(super::super::demo::entries()));
                self.scroll_y.set(self.max_scroll.peek());
                self.refresh();
            }
            _ => return false,
        }
        true
    }

    pub(in crate::tui::sigil_view) fn open_sessions(&self, tree: bool) {
        if self.model.get().busy_since.is_some() {
            self.push_entry(Entry::Error(
                "Cancel the active turn before resuming a session.".to_string(),
            ));
            self.refresh();
        } else {
            self.sessions.start(&self.model.get().session, tree);
            drop(self.cmd.send(Cmd::ListSessions));
        }
    }

    fn model_command(&self, argument: Option<&str>) {
        let mut words: Vec<&str> = argument.unwrap_or_default().split_whitespace().collect();
        if let Some(level) = words
            .last()
            .copied()
            .filter(|word| crate::provider::Effort::parse(word).is_some())
        {
            words.pop();
            self.effort_command(level);
        }
        match words.as_slice() {
            [] if argument.is_none() => self.begin_model_edit(),
            [] => {}
            [model] => {
                let endpoint = self.model.peek().snapshot.endpoint.clone();
                let own = crate::catalog::known(&endpoint, model).is_some();
                drop(self.cmd.send(match model.split_once('/') {
                    Some((name, id)) if !own && crate::endpoint::is_named(name) => {
                        Cmd::SetSeat(name.to_string(), id.to_string())
                    }
                    Some(_) | None => Cmd::SetModel((*model).to_string()),
                }));
            }
            [..] => {
                self.push_entry(Entry::Error("/model NAME [LEVEL]".to_string()));
                self.refresh();
            }
        }
    }

    pub(in crate::tui::sigil_view) fn toggle_mode(&self) {
        let mode = {
            let snapshot = &self.model.get().snapshot;
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
        self.model_mut(|model| {
            model.snapshot.plan = next == crate::RunMode::Plan;
            model.snapshot.work = next == crate::RunMode::Work;
            model.snapshot.goal = next == crate::RunMode::Goal;
        });
        drop(self.cmd.send(Cmd::SetRunMode(next)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_verify(&self) {
        let on = self.model.get().snapshot.verify.is_none();
        drop(self.cmd.send(Cmd::SetVerify(on)));
    }

    pub(in crate::tui::sigil_view) fn cycle_endpoint(&self) {
        self.cycle_seat(true);
    }

    pub(in crate::tui::sigil_view) fn cycle_seat(&self, forward: bool) {
        let current = self.model.get().snapshot.endpoint.clone();
        match crate::endpoint::step(&current, forward) {
            Some(next) => drop(self.cmd.send(Cmd::SetEndpoint(next))),
            None => {
                self.model_mut(|model| {
                    model.entries.push(Entry::System(
                        "only one ready seat; `summonr login NAME` adds another".to_string(),
                    ))
                });
                self.refresh();
            }
        }
    }
}
