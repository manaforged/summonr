use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn update(&self, msg: Msg) {
        match msg {
            Msg::Secret(request) => self.secret_open(request),
            Msg::Clipboard(read) => self.pasted(read),
            Msg::LoginStep {
                text,
                url,
                code,
                asking,
            } => self.auth_progress(text, url, code, asking),
            Msg::LoginDone {
                name,
                result,
                rejected,
            } => self.auth_done(&name, result, rejected),
            Msg::OpenHelp => self.help.show(),
            Msg::OpenLogin(note) => self.login_opened(note),
            msg => self.update_loaded(msg),
        }
    }

    fn update_loaded(&self, msg: Msg) {
        match msg {
            Msg::Prompts { session, result } => self.prompts_loaded(session, result),
            Msg::Restored(inputs) => self.restore(inputs),
            Msg::Saved { file, text, result } => self.save_done(file, text, result),
            Msg::Resume(id) => self.open_session(crate::tui::hub::Open::Resume(id)),
            Msg::Sessions(rows) => self.sessions.update(rows),
            Msg::Usage(rows) => self.usage.update(rows),
            msg @ Msg::Models { .. } => self.models_loaded(msg),
            Msg::ModelNote(note) => self.picker.note.set(note),
            Msg::Defaulted(seat) => self.defaulted(seat),
            Msg::Workflow { name, names, seats } => self.workflow_loaded(name, names, seats),
            msg @ Msg::Patch { .. } => self.patch_loaded(msg),
            msg => self.apply_msg(msg),
        }
    }

    fn apply_msg(&self, msg: Msg) {
        let reveal_plan = self.prepare(&msg);
        let mut alive = true;
        self.model_mut(|model| alive = model.apply(msg));
        if !alive {
            self.quit.set(true);
        }
        if reveal_plan {
            self.reveal_plan();
        }
    }

    fn prepare(&self, msg: &Msg) -> bool {
        match msg {
            Msg::Event(crate::Event::Work { progress }) => {
                let reveal = self.model.get().ledger.is_empty()
                    && progress
                        .ledger
                        .iter()
                        .any(|line| crate::tui::sigil_view::model::itemized(line));
                self.load();
                reveal
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
            Msg::Event(crate::Event::Approval { detail, persist }) => {
                self.approval
                    .set(detail.clone().map(|detail| (detail, *persist)));
                false
            }
            Msg::Event(crate::Event::Elicit { form }) => {
                self.form_open(form.clone());
                false
            }
            Msg::Diff(_) => {
                if let Some(path) = self.diff_view.peek() {
                    self.model_mut(|model| model.patch = None);
                    drop(self.cmd.send(Cmd::Patch(self.root(), path)));
                }
                false
            }
            Msg::Reset => {
                self.approval.set(None);
                self.form.set(None);
                false
            }
            Msg::Busy(None) => {
                self.approval.set(None);
                self.form.set(None);
                self.secret_finish(false);
                drop(self.cmd.send(Cmd::Offers));
                false
            }
            Msg::Changed(paths) => {
                self.changed_files(paths);
                false
            }
            _ => false,
        }
    }

    fn changed_files(&self, paths: &[String]) {
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

    fn patch_loaded(&self, msg: Msg) {
        let Msg::Patch { root, path, result } = &msg else {
            return;
        };
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
        self.model_mut(|model| {
            model.apply(msg);
        });
    }

    fn models_loaded(&self, msg: Msg) {
        let Msg::Models {
            request,
            mut rows,
            facts,
            groups,
            causes,
            pinned,
            recent,
            loading,
            error,
        } = msg
        else {
            return;
        };
        if request != self.picker.request.get() {
            return;
        }
        if !self.picker.open.peek() {
            self.picker.groups.update(|held| held.extend(groups));
            if self.auth.open.peek() {
                self.picker.choices.set(std::rc::Rc::new(rows));
                self.picker.facts.set(std::rc::Rc::new(facts));
                self.regroup_auth();
            }
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

    fn login_opened(&self, note: String) {
        self.begin_auth(None);
        if self.unseated() {
            let rows = self.filtered_auth();
            let ready = crate::endpoint::set_up();
            let at = rows
                .iter()
                .position(|choice| ready.contains(&choice.name))
                .or_else(|| rows.iter().position(|choice| !choice.keyless()))
                .unwrap_or(0);
            self.auth.sel.set(at);
        }
        self.auth
            .step
            .set(Login::List(super::auth::Note::plain(note)));
    }

    fn prompts_loaded(
        &self,
        session: Option<String>,
        result: std::result::Result<(Vec<crate::session::Prompt>, Vec<String>), String>,
    ) {
        let current = match session {
            Some(id) => self.search.is_branch() && id == self.model.get().session,
            None => !self.search.is_branch(),
        };
        if !(self.search.open.peek() && self.search.is_history() && current) {
            return;
        }
        let result = result.map(|(prompts, errors)| {
            let skipped = errors.len();
            self.model_mut(|model| model.entries.extend(errors.into_iter().map(Entry::Error)));
            (prompts, skipped)
        });
        self.search.update(result);
    }

    fn save_done(&self, file: PathBuf, text: String, result: std::result::Result<(), String>) {
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
                    "saved {} · {}",
                    file.display(),
                    crate::text::plural(lines, "line")
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

    fn workflow_loaded(
        &self,
        name: Option<String>,
        names: Vec<String>,
        mut seats: Vec<Option<String>>,
    ) {
        self.workflow.name.set(name.unwrap_or_default());
        self.workflow.names.set(names);
        seats.resize(Role::ALL.len(), None);
        self.workflow.seats.set(seats);
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

    pub(in crate::tui::sigil_view) fn approval_answer(&self, answer: crate::runtime::GrantAnswer) {
        self.approval.set(None);
        self.steer_answer(answer.token().to_string());
    }

    pub(in crate::tui::sigil_view) fn steer_answer(&self, text: String) {
        drop(self.cmd.send(Cmd::Steer(UserInput::from(text))));
    }
}
