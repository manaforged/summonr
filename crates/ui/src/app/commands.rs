use super::*;

impl App {
    pub(crate) fn suggesting(&self) -> bool {
        !self.settings_open.peek()
            && !self.workflow.open.peek()
            && !self.picker.open.peek()
            && self
                .commands
                .open(&self.draft.peek(), self.caret.peek(), &self.model.get())
    }

    pub(crate) fn completion(&self, value: &str) {
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
        let model = self.model.get();
        let peer = model.peers.iter().any(|agent| agent.answers_to(value))
            || super::suggest::offered(&model, value);
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

    pub(crate) fn local(&self, line: &str, argument: Option<&str>) -> bool {
        let command = summonr_agent::parse_command(line);
        match command {
            Some(summonr_agent::SlashCommand::Help) => self.help.show(),
            Some(summonr_agent::SlashCommand::Retry) => self.retry(),
            Some(summonr_agent::SlashCommand::Search) => self.search.start(argument.unwrap_or("")),
            Some(summonr_agent::SlashCommand::History) => {
                self.prompt_history(argument.unwrap_or(""))
            }
            Some(summonr_agent::SlashCommand::Fork) if argument.is_some() => {
                self.fork_command(argument.unwrap_or(""));
            }
            Some(summonr_agent::SlashCommand::Resume) if argument.is_none() => {
                self.open_sessions(false)
            }
            Some(summonr_agent::SlashCommand::New) => {
                self.open_session(summonr_agent::hub::Open::New)
            }
            Some(summonr_agent::SlashCommand::Usage) if argument.is_none() => {
                if !self.window.show_usage(self) {
                    self.open_usage();
                }
            }
            settings => return self.local_settings(settings, argument),
        }
        true
    }

    fn fork_command(&self, argument: &str) {
        if self.model.get().busy_since.is_some() {
            self.push_entry(Entry::Error(
                "wait for the active operation or cancel it before forking".to_string(),
            ));
            self.refresh();
        } else {
            self.search.branch(argument);
            drop(
                self.cmd
                    .send(Cmd::ListPrompts(Some(self.model.get().session.clone()))),
            );
        }
    }

    fn local_settings(
        &self,
        command: Option<summonr_agent::SlashCommand>,
        argument: Option<&str>,
    ) -> bool {
        match command {
            Some(summonr_agent::SlashCommand::Settings) => {
                self.hotkeys.open.set(false);
                self.settings_group.set(None);
                self.settings.select(0);
                self.settings_open.set(true);
            }
            Some(summonr_agent::SlashCommand::Login) => self.begin_auth(argument),
            Some(summonr_agent::SlashCommand::Model) => self.model_command(argument),
            Some(summonr_agent::SlashCommand::Effort) => match argument {
                Some(level) => self.effort_command(level),
                None => {
                    let snapshot = self.model.peek().snapshot.clone();
                    self.effort.show(snapshot.effort, &snapshot.levels);
                }
            },
            Some(summonr_agent::SlashCommand::Workflow) if argument.is_none() => {
                self.open_workflow();
            }
            files => return self.local_files(files, argument),
        }
        true
    }

    fn local_files(
        &self,
        command: Option<summonr_agent::SlashCommand>,
        argument: Option<&str>,
    ) -> bool {
        match command {
            Some(summonr_agent::SlashCommand::Pane) => self.pane_command(argument),
            Some(summonr_agent::SlashCommand::Mcp) => self.pane_command(Some(Pane::Mcp.name())),
            Some(summonr_agent::SlashCommand::Diff) => self.diff_command(argument),
            Some(summonr_agent::SlashCommand::Copy) => self.copy_chat(argument),
            Some(summonr_agent::SlashCommand::Export) => {
                drop(self.cmd.send(Cmd::Export(argument.map(str::to_string))));
            }
            Some(summonr_agent::SlashCommand::View) => self.view_command(argument),
            Some(summonr_agent::SlashCommand::Demo) => {
                self.model_mut(|model| {
                    model.entries.extend(super::super::demo::entries());
                    model.agents.extend(super::super::demo::agents());
                });
                self.scroll_y.set(self.max_scroll.peek());
                self.refresh();
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn open_sessions(&self, tree: bool) {
        if self.model.get().busy_since.is_some() {
            self.push_entry(Entry::Error(
                "cancel the active turn before resuming a session".to_string(),
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
            .filter(|word| summonr_agent::provider::Effort::parse(word).is_some())
        {
            words.pop();
            self.effort_command(level);
        }
        match words.as_slice() {
            [] if argument.is_none() => self.begin_model_edit(),
            [] => {}
            [model] => {
                let endpoint = self.model.peek().snapshot.endpoint.clone();
                let own = summonr_agent::catalog::known(&endpoint, model).is_some();
                drop(self.cmd.send(match model.split_once('/') {
                    Some((name, id)) if !own && summonr_agent::endpoint::is_named(name) => {
                        Cmd::SelectModel(name.to_string(), id.to_string())
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

    pub(crate) fn cycle_effort(&self, delta: isize) {
        let levels = self.model.get().snapshot.levels.clone();
        if levels.is_empty() {
            return;
        }
        let next = match self.model.get().snapshot.effort.map(|at| at.clamp(&levels)) {
            Some(at) => at.shifted(&levels, delta),
            None if delta < 0 => levels[levels.len() - 1],
            None => levels[0],
        };
        self.model_mut(|model| model.snapshot.effort = Some(next));
        drop(self.cmd.send(Cmd::SetEffort(Some(next))));
        self.refresh();
    }

    pub(crate) fn toggle_verify(&self) {
        let on = self.model.get().snapshot.verify.is_none();
        drop(self.cmd.send(Cmd::SetVerify(on)));
    }
}
