use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn update(&self, msg: Msg) {
        let msg = match msg {
            Msg::Secret(request) => {
                self.secret_open(request);
                return;
            }
            Msg::Clipboard(read) => {
                self.pasted(read);
                return;
            }
            Msg::LoginStep {
                text,
                url,
                code,
                asking,
                ..
            } => {
                self.auth_progress(text, url, code, asking);
                return;
            }
            Msg::LoginDone { name, result } => {
                self.auth_done(&name, result);
                return;
            }
            msg => msg,
        };
        let reveal_plan = match &msg {
            Msg::Event(crate::Event::Work { progress }) => {
                self.model.get().ledger.is_empty()
                    && progress
                        .ledger
                        .iter()
                        .any(|line| crate::tui::sigil_view::model::itemized(line))
            }
            Msg::Event(crate::Event::ToolCall {
                name, arguments, ..
            }) if crate::tools::Builtin::is(name, crate::tools::Builtin::Todo) => {
                crate::tools::todos_from_args(arguments).is_ok_and(|items| !items.is_empty())
            }
            Msg::Event(crate::Event::Child { event, .. }) => matches!(
                event.as_ref(),
                crate::Event::ToolCall { name, .. } if crate::tools::Builtin::is(name, crate::tools::Builtin::Todo)
            ),
            _ => false,
        };
        if matches!(msg, Msg::Event(crate::Event::Work { .. })) {
            self.load();
        }
        if matches!(msg, Msg::Diff(_))
            && let Some(path) = self.diff_view.peek()
        {
            self.model_mut(|model| model.patch = None);
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
            let viewing = self.model.get().view.as_ref().map(|view| view.path.clone());
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
            Msg::OpenLogin(note) => {
                self.begin_auth(None);
                if self.unseated() {
                    self.auth.sel.set(0);
                }
                self.auth.step.set(Login::List(note));
            }
            Msg::Prompts { session, result } => {
                let current = match session {
                    Some(id) => self.search.is_branch() && id == self.model.get().session,
                    None => !self.search.is_branch(),
                };
                if self.search.open.peek() && self.search.is_history() && current {
                    let result = result.map(|(prompts, errors)| {
                        let skipped = errors.len();
                        self.model_mut(|model| {
                            model.entries.extend(errors.into_iter().map(Entry::Error))
                        });
                        (prompts, skipped)
                    });
                    self.search.update(result);
                }
            }
            Msg::Restored(inputs) => self.restore(inputs),
            Msg::Saved { file, text, result } => {
                let editing = self.view_edit.peek();
                let mut saved = false;
                self.model_mut(|model| match result {
                    Ok(()) => {
                        let viewed = model
                            .view
                            .as_mut()
                            .filter(|view| view.file == file)
                            .map(|view| view.text = text.clone())
                            .is_some();
                        let lines = text.lines().count();
                        model.entries.push(Entry::System(format!(
                            "saved {} · {lines} line{}",
                            file.display(),
                            if lines == 1 { "" } else { "s" }
                        )));
                        saved = viewed && editing;
                    }
                    Err(error) => {
                        model.entries.push(Entry::Error(format!(
                            "cannot save {}: {error}",
                            file.display()
                        )));
                    }
                });
                if saved {
                    self.view_saved.set(text);
                    self.discard_armed.set(false);
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
                causes,
                pinned,
                recent,
                loading,
                error,
            } => {
                if request != self.picker.request.get() {
                    return;
                }
                if !self.picker.open.peek() {
                    self.picker.groups.update(|held| held.extend(groups));
                    return;
                }
                let snapshot = self.model.peek().snapshot.clone();
                let current = Seat::new(snapshot.endpoint, snapshot.model);
                let seated = !current.model.is_empty();
                let mut counts = [usize::from(seated), pinned, recent];
                if let Some(at) = rows.iter().position(|row| *row == current) {
                    rows.remove(at);
                    let slot = if at < pinned { 1 } else { 2 };
                    counts[slot] = counts[slot].saturating_sub(usize::from(at < pinned + recent));
                }
                if seated {
                    rows.insert(0, current.clone());
                }
                let selected = self.picked_model();
                self.picker.facts.set(Rc::new(facts));
                self.picker.causes.update(|held| {
                    held.retain(|name, _| !groups.contains_key(name));
                    held.extend(causes);
                });
                self.picker.groups.update(|held| held.extend(groups));
                self.picker.choices.set(Rc::new(rows));
                self.picker.recent.set(counts);
                self.rank();
                let shown = self.picker.shown.peek();
                let sel = [selected.unwrap_or_default(), current]
                    .iter()
                    .find_map(|wanted| shown.iter().position(|row| row == wanted))
                    .unwrap_or(0);
                self.picker.sel.set(sel);
                self.picker.loading.set(loading);
                self.picker.error.set(error);
            }
            Msg::ModelNote(note) => self.picker.note.set(note),
            Msg::Defaulted(seat) => self.defaulted(seat),
            Msg::Workflow { name, names, seats } => {
                self.workflow.name.set(name.unwrap_or_default());
                self.workflow.names.set(names);
                let mut seats = seats;
                seats.resize(Role::ALL.len(), None);
                self.workflow.seats.set(seats);
            }
            msg @ Msg::Patch { .. } => {
                self.model_mut(|model| {
                    model.apply(msg);
                });
            }
            msg => {
                let mut alive = true;
                self.model_mut(|model| alive = model.apply(msg));
                if !alive {
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
        if layout.secondary.is_some() || self.viewport.peek().0 < 120 {
            return;
        }
        self.layout.set(Layout {
            secondary: Some(Pane::Ledger),
            share: 20,
        });
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
