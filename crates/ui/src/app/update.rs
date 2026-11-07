use super::*;

impl App {
    pub(crate) fn update(&self, msg: Msg) {
        match msg {
            Msg::Fact(Fact::Transcript(lines)) => {
                lines.into_iter().for_each(|msg| self.update(msg))
            }
            Msg::Fact(Fact::Secret(request)) => self.secret_open(request),
            Msg::Fact(Fact::Moved(folder)) => {
                self.open_session(summonr_agent::hub::Open::Folder(folder));
            }
            Msg::Fact(Fact::Clipboard(read)) => self.pasted(read),
            Msg::Fact(Fact::LoginStep {
                text,
                url,
                code,
                asking,
            }) => self.auth_progress(text, url, code, asking),
            Msg::Fact(Fact::LoginDone {
                name,
                result,
                rejected,
            }) => self.auth_done(&name, result, rejected),
            Msg::Intent(Intent::OpenHelp) => self.help.show(),
            Msg::Intent(Intent::OpenLogin(note)) => self.login_opened(note),
            msg => self.update_loaded(msg),
        }
    }

    fn update_loaded(&self, msg: Msg) {
        match msg {
            Msg::Fact(Fact::Prompts { session, result }) => self.prompts_loaded(session, result),
            Msg::Fact(Fact::Restored(inputs)) => self.restore(inputs),
            Msg::Fact(Fact::Saved { file, text, result }) => self.save_done(file, text, result),
            Msg::Intent(Intent::Resume(id)) => {
                self.open_session(summonr_agent::hub::Open::Resume(id))
            }
            Msg::Fact(Fact::Sessions(rows)) => self.sessions.update(rows),
            Msg::Fact(Fact::Usage(rows)) => self.usage.update(rows),
            msg @ Msg::Fact(Fact::Models { .. }) => self.models_loaded(msg),
            Msg::Fact(Fact::ModelNote(note)) => self.picker.note.set(note),
            Msg::Fact(Fact::Defaulted(selection)) => self.defaulted(selection),
            Msg::Fact(Fact::Workflow {
                name,
                names,
                selections,
            }) => self.workflow_loaded(name, names, selections),
            msg @ Msg::Fact(Fact::Patch { .. }) => self.patch_loaded(msg),
            Msg::Fact(
                Fact::Diff(_, root) | Fact::DiffFailed(_, root) | Fact::View { root, .. },
            ) if root != self.root() => {}
            msg => self.apply_msg(msg),
        }
    }

    fn apply_msg(&self, msg: Msg) {
        self.prepare(&msg);
        let mut alive = true;
        self.model_mut(|model| alive = model.apply(msg));
        if !alive {
            self.quit.set(true);
        }
    }

    fn prepare(&self, msg: &Msg) {
        self.window.call_attention(self, msg);
        match msg {
            Msg::Fact(Fact::Event(summonr_agent::Event::Work { .. })) => self.load(),
            Msg::Fact(Fact::Event(summonr_agent::Event::Approval {
                detail,
                persist,
                dialog,
            })) => {
                self.approval.set(detail.clone().map(|detail| Asked {
                    detail,
                    persist: *persist,
                    dialog: *dialog,
                }));
            }
            Msg::Fact(Fact::Event(summonr_agent::Event::Elicit { form, dialog })) => {
                self.form_open(form.clone(), *dialog);
            }
            Msg::Fact(Fact::Settings(snapshot)) => self.move_to(&snapshot.workspace),
            Msg::Fact(Fact::Diff(diff, _)) => {
                if self.diff_session.peek()
                    && self.root().is_none()
                    && diff.base != summonr_agent::diff::SESSION_BASE
                {
                    self.request_diff();
                }
                if let Some(path) = self.diff_view.peek() {
                    self.model_mut(|model| model.patch = None);
                    drop(self.cmd.send(Cmd::Patch(self.root(), path)));
                }
            }
            Msg::Intent(Intent::Reset) => {
                self.approval.set(None);
                self.form_close();
            }
            Msg::Fact(Fact::Busy(None)) => {
                self.approval.set(None);
                self.form_close();
                self.secret_finish(false);
                drop(self.cmd.send(Cmd::Offers));
            }
            Msg::Fact(Fact::Changed(paths)) => {
                self.changed_files(paths);
            }
            _ => {}
        }
    }

    fn changed_files(&self, paths: &[String]) {
        let viewing = self.model.get().view.as_ref().map(|view| view.path.clone());
        match self.layout.peek().secondary {
            Some(Pane::Files | Pane::Diff) => self.load(),
            Some(Pane::View) => {
                if let Some(path) = viewing.filter(|path| paths.contains(path)) {
                    drop(self.cmd.send(Cmd::View(self.root(), path)));
                }
            }
            _ => {}
        }
    }

    fn patch_loaded(&self, msg: Msg) {
        let Msg::Fact(Fact::Patch { root, path, result }) = &msg else {
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
        let Msg::Fact(Fact::Models {
            request,
            mut rows,
            facts,
            groups,
            causes,
            pinned,
            recent,
            loading,
            error,
        }) = msg
        else {
            return;
        };
        if request != self.picker.request.get() {
            return;
        }
        if !self.picker.open.peek() {
            batch(|| {
                self.picker.groups.update(|held| held.extend(groups));
                if self.auth.open.peek() {
                    self.picker.choices.set(Rc::new(rows));
                    self.picker.facts.set(Rc::new(facts));
                }
            });
            return;
        }
        let snapshot = self.model.peek().snapshot.clone();
        let current = ModelId::new(snapshot.endpoint, snapshot.model);
        let selected = !current.model.is_empty();
        let mut counts = [usize::from(selected), pinned, recent];
        if let Some(at) = rows.iter().position(|row| *row == current) {
            rows.remove(at);
            let slot = if at < pinned { 1 } else { 2 };
            counts[slot] = counts[slot].saturating_sub(usize::from(at < pinned + recent));
        }
        if selected {
            rows.insert(0, current.clone());
        }
        let selected = self.picked_model();
        batch(|| {
            self.picker.facts.set(Rc::new(facts));
            self.picker.causes.update(|held| {
                held.retain(|name, _| !groups.contains_key(name));
                held.extend(causes);
            });
            self.picker.groups.update(|held| held.extend(groups));
            self.picker.choices.set(Rc::new(rows));
            self.picker.recent.set(counts);
        });
        let shown = self.picker.shown.peek();
        let sel = [selected.unwrap_or_default(), current]
            .iter()
            .find_map(|wanted| shown.iter().position(|row| row == wanted))
            .unwrap_or(0);
        self.picker.list.cursor.set(sel);
        self.picker.loading.set(loading);
        self.picker.error.set(error);
    }

    fn login_opened(&self, note: String) {
        self.begin_auth(None);
        if self.unconfigured() {
            let rows = self.filtered_auth();
            let ready = summonr_agent::endpoint::set_up();
            let at = rows
                .iter()
                .position(|choice| ready.contains(&choice.name))
                .or_else(|| rows.iter().position(|choice| !choice.keyless()))
                .unwrap_or(0);
            self.auth.list.cursor.set(at);
        }
        self.auth
            .step
            .set(Login::List(super::auth::Note::plain(note)));
    }

    fn prompts_loaded(
        &self,
        session: Option<String>,
        result: std::result::Result<(Vec<summonr_agent::session::Prompt>, Vec<String>), String>,
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
                    summonr_core::text::plural(lines, "line")
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
            self.discard_armed.set(None);
        }
    }

    fn workflow_loaded(
        &self,
        name: Option<String>,
        names: Vec<String>,
        mut selections: Vec<Option<String>>,
    ) {
        self.workflow.name.set(name.unwrap_or_default());
        self.workflow.names.set(names);
        selections.resize(Role::ALL.len(), None);
        self.workflow.selections.set(selections);
    }

    pub(crate) fn approval_answer(&self, answer: summonr_agent::runtime::GrantAnswer) {
        let Some(asked) = self.approval.peek() else {
            return;
        };
        self.approval.set(None);
        self.steer_answer(asked.dialog, answer.token().to_string());
    }

    pub(crate) fn steer_answer(&self, dialog: u64, text: String) {
        drop(self.cmd.send(Cmd::Answer(dialog, text)));
    }
}
