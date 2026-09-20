use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn update(&self, msg: Msg) {
        let msg = match msg {
            Msg::Secret(request) => {
                self.secret_open(request);
                return;
            }
            msg => msg,
        };
        let reveal_plan = match &msg {
            Msg::Event(crate::Event::Work { progress }) => {
                self.model.borrow().ledger.is_empty()
                    && progress
                        .ledger
                        .iter()
                        .any(|line| crate::tui::sigil_view::model::itemized(line))
            }
            Msg::Event(crate::Event::ToolCall {
                name, arguments, ..
            }) if name == "todo" => {
                crate::tools::todos_from_args(arguments).is_ok_and(|items| !items.is_empty())
            }
            Msg::Event(crate::Event::Child { event, .. }) => matches!(
                event.as_ref(),
                crate::Event::ToolCall { name, .. } if name == "todo"
            ),
            _ => false,
        };
        if matches!(msg, Msg::Event(crate::Event::Work { .. })) {
            self.load();
        }
        if matches!(msg, Msg::Diff(_))
            && let Some(path) = self.diff_view.peek()
        {
            self.model.borrow_mut().patch = None;
            drop(self.cmd.send(Cmd::Patch(self.root(), path)));
        }
        if let Msg::Event(crate::Event::Approval { detail, persist }) = &msg {
            self.approval
                .set(detail.clone().map(|detail| (detail, *persist)));
        }
        if matches!(msg, Msg::Busy(None)) {
            self.approval.set(None);
            self.secret_finish(false);
        }
        if let Msg::Patch { root, path, result } = &msg {
            if self
                .copy_pending
                .peek()
                .is_some_and(|(wanted, file)| &wanted == root && file == *path)
            {
                self.copy_pending.set(None);
                self.patch_copy(result.clone());
            }
            if *root != self.root() || self.diff_view.peek().as_ref() != Some(path) {
                return;
            }
        }
        if let Msg::Changed(paths) = &msg {
            let viewing = self
                .model
                .borrow()
                .view
                .as_ref()
                .map(|view| view.path.clone());
            match self.layout.peek().secondary {
                Some(Pane::Files) => drop(self.cmd.send(Cmd::ListFiles)),
                Some(Pane::Diff) => drop(self.cmd.send(Cmd::Diff(self.root()))),
                Some(Pane::View) => {
                    if let Some(path) = viewing.filter(|path| paths.contains(path)) {
                        drop(self.cmd.send(Cmd::View(self.root(), path)));
                    }
                }
                _ => {}
            }
        }
        match msg {
            Msg::OpenHelp => self.help.show(),
            Msg::Prompts { session, result } => {
                let current = match session {
                    Some(id) => self.search.is_branch() && id == self.model.borrow().session,
                    None => !self.search.is_branch(),
                };
                if self.search.open.peek() && self.search.is_history() && current {
                    let result = result.map(|(prompts, errors)| {
                        let skipped = errors.len();
                        self.model
                            .borrow_mut()
                            .entries
                            .extend(errors.into_iter().map(Entry::Error));
                        (prompts, skipped)
                    });
                    self.search.update(result);
                }
            }
            Msg::Restored(inputs) => self.restore(inputs),
            Msg::Saved { file, text, result } => {
                let mut model = self.model.borrow_mut();
                match result {
                    Ok(()) => {
                        if let Some(view) = model.view.as_mut().filter(|view| view.file == file) {
                            view.text = text.clone();
                            if self.view_edit.peek() {
                                self.view_saved.set(text.clone());
                                self.discard_armed.set(false);
                            }
                        }
                        let lines = text.lines().count();
                        model.entries.push(Entry::System(format!(
                            "saved {} · {lines} line{}",
                            file.display(),
                            if lines == 1 { "" } else { "s" }
                        )));
                    }
                    Err(error) => model.entries.push(Entry::Error(format!(
                        "cannot save {}: {error}",
                        file.display()
                    ))),
                }
            }
            Msg::Resume(id) => self.request_resume(id),
            Msg::Sessions(rows) => self.sessions.update(rows),
            Msg::Usage(rows) => self.usage.update(rows),
            Msg::Models {
                request,
                mut rows,
                facts,
                groups,
                recent,
                loading,
                error,
            } => {
                if !self.editing_model.peek() || request != self.model_request.get() {
                    return;
                }
                let snapshot = self.model.borrow().snapshot.clone();
                let current = format!("{}\t{}", snapshot.endpoint, snapshot.model);
                let mut recent = recent.min(rows.len());
                if rows.iter().any(|row| {
                    row.split_once('\t')
                        .is_some_and(|(name, _)| name == snapshot.endpoint)
                }) && !rows.contains(&current)
                {
                    rows.insert(0, current);
                    recent = recent.saturating_add(1);
                }
                let selected = self.filtered_models().get(self.model_sel.peek()).cloned();
                self.model_facts.set(facts);
                self.model_groups.set(groups);
                self.model_choices.set(rows);
                let sel = selected
                    .and_then(|row| {
                        self.filtered_models()
                            .iter()
                            .position(|candidate| *candidate == row)
                    })
                    .unwrap_or(0);
                self.model_recent.set(recent);
                self.model_sel.set(sel);
                self.model_loading.set(loading);
                self.model_error.set(error);
            }
            Msg::Workflow { name, names, seats } => {
                self.workflow_name.set(name.unwrap_or_default());
                self.workflow_names.set(names);
                let mut seats = seats;
                seats.resize(Role::ALL.len(), None);
                self.workflow_seats.set(seats);
            }
            msg @ Msg::Patch { .. } => {
                self.model.borrow_mut().apply(msg);
                self.constrain();
            }
            msg => {
                if !self.model.borrow_mut().apply(msg) {
                    self.quit.set(true);
                }
            }
        }
        if reveal_plan {
            self.reveal_plan();
        }
    }

    pub(in crate::tui::sigil_view) fn reveal_plan(&self) {
        let layout = self.layout.peek();
        if layout.secondary.is_some() {
            return;
        }
        self.layout.set(Layout {
            secondary: Some(Pane::Ledger),
            share: 20,
        });
        self.constrain();
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn approval_answer(&self, token: &str) {
        self.approval.set(None);
        drop(
            self.cmd
                .send(Cmd::Steer(UserInput::from(token.to_string()))),
        );
    }
}
