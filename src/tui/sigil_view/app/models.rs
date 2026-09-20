use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn begin_model_edit(&self) {
        self.replace_model_draft(String::new());
        self.models_focus.set(self.models_focus.peek() + 1);
        self.model_choices.set(Vec::new());
        self.model_sel.set(0);
        self.model_recent.set(0);
        self.model_error.set(String::new());
        self.model_loading.set(true);
        self.editing_model.set(true);
        let request = self.model_request.get().wrapping_add(1);
        self.model_request.set(request);
        let (sender, cancel) = oneshot::channel();
        *self.model_cancel.borrow_mut() = Some(sender);
        if self.cmd.send(Cmd::ListModels { request, cancel }).is_err() {
            self.model_loading.set(false);
            self.model_error
                .set("Model discovery is unavailable.".to_string());
        }
    }

    pub(in crate::tui::sigil_view) fn refresh_models(&self) {
        self.model_error.set(String::new());
        self.model_loading.set(true);
        let request = self.model_request.get().wrapping_add(1);
        self.model_request.set(request);
        let (sender, cancel) = oneshot::channel();
        *self.model_cancel.borrow_mut() = Some(sender);
        if self.cmd.send(Cmd::ListModels { request, cancel }).is_err() {
            self.model_loading.set(false);
            self.model_error
                .set("Model discovery is unavailable.".to_string());
        }
    }

    pub(in crate::tui::sigil_view) fn replace_model_draft(&self, draft: String) {
        if self.model_draft.peek() != draft {
            self.model_caret.set(draft.len());
            self.model_draft.set(draft);
        }
        self.model_sel.set(0);
    }

    pub(in crate::tui::sigil_view) fn commit_model_edit(&self) {
        let choices = self.filtered_models();
        let selected = self.model_sel.peek().min(choices.len().saturating_sub(1));
        let Some(picked) = choices.get(selected).cloned() else {
            return;
        };
        if let Some(name) = picked.split_once('\t').map(|(name, _)| name)
            && self.model_groups.peek().get(name).map(String::as_str)
                == Some(crate::tui::worker::NEEDS_LOGIN)
        {
            let name = name.to_string();
            self.cancel_model_edit();
            drop(self.cmd.send(Cmd::Login(name)));
            return;
        }
        let target = self.model_for.peek();
        self.cancel_model_edit();
        if let Some(role) = target.and_then(|index| Role::ALL.get(index)) {
            let current = self.model.borrow().snapshot.endpoint.clone();
            let (endpoint, model) = picked
                .split_once('\t')
                .map_or((current, picked.clone()), |(name, model)| {
                    (name.to_string(), model.to_string())
                });
            drop(self.cmd.send(Cmd::SeatRole(*role, endpoint, model)));
            return;
        }
        if let Some((name, model)) = picked.split_once('\t') {
            let current = self.model.borrow().snapshot.endpoint.clone();
            if name != current {
                drop(
                    self.cmd
                        .send(Cmd::SetSeat(name.to_string(), model.to_string())),
                );
            } else {
                drop(self.cmd.send(Cmd::SetModel(model.to_string())));
            }
        } else {
            drop(self.cmd.send(Cmd::SetModel(picked)));
        }
    }

    pub(in crate::tui::sigil_view) fn cancel_model_edit(&self) {
        drop(self.model_cancel.borrow_mut().take());
        self.editing_model.set(false);
        self.model_loading.set(false);
        self.model_for.set(None);
        self.replace_model_draft(String::new());
    }

    pub(in crate::tui::sigil_view) fn move_model(&self, delta: isize) {
        let last = self.filtered_models().len().saturating_sub(1);
        self.model_sel
            .set(self.model_sel.peek().saturating_add_signed(delta).min(last));
    }

    pub(in crate::tui::sigil_view) fn filtered_models(&self) -> Vec<String> {
        let query = self.model_draft.peek().to_lowercase();
        self.model_choices
            .peek()
            .into_iter()
            .filter(|model| {
                let model = model.to_lowercase();
                query.split_whitespace().all(|word| model.contains(word))
            })
            .collect()
    }
}
