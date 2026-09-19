use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn begin_auth(&self, focus: Option<&str>) {
        let choices: Vec<_> = crate::endpoint::names()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|name| crate::endpoint::auth_info(&name).ok())
            .collect();
        let current = self.model.borrow().snapshot.endpoint.clone();
        if let Some(name) = focus
            && !choices.iter().any(|choice| choice.name == name)
        {
            self.model.borrow_mut().entries.push(Entry::Error(format!(
                "Provider `{name}` is unavailable. Use /login to choose a provider."
            )));
            self.splash.set(false);
            self.refresh();
            return;
        }
        let focus = focus.unwrap_or(&current);
        let selected = choices
            .iter()
            .position(|choice| choice.name == focus)
            .or_else(|| choices.iter().position(|choice| choice.name == current))
            .unwrap_or(0);
        self.auth_choices.set(choices);
        self.auth_sel.set(selected);
        self.auth_entering.set(false);
        self.replace_auth_key(String::new());
        self.auth_open.set(true);
    }

    pub(in crate::tui::sigil_view) fn move_auth(&self, delta: isize) {
        if self.auth_entering.peek() {
            return;
        }
        let last = self.auth_choices.peek().len().saturating_sub(1);
        self.auth_sel
            .set(self.auth_sel.peek().saturating_add_signed(delta).min(last));
    }

    pub(in crate::tui::sigil_view) fn commit_auth(&self) {
        let choices = self.auth_choices.peek();
        let Some(choice) = choices.get(self.auth_sel.peek().min(choices.len().saturating_sub(1)))
        else {
            return;
        };
        if choice.device_login {
            self.auth_open.set(false);
            drop(self.cmd.send(Cmd::Login(choice.name.clone())));
            return;
        }
        if !self.auth_entering.peek() {
            self.auth_entering.set(true);
            self.replace_auth_key(String::new());
            return;
        }
        let key = self.auth_key.peek();
        if key.trim().is_empty() {
            return;
        }
        self.auth_open.set(false);
        self.auth_entering.set(false);
        self.replace_auth_key(String::new());
        drop(self.cmd.send(Cmd::SetKey(choice.name.clone(), key)));
    }

    pub(in crate::tui::sigil_view) fn cancel_auth(&self) {
        if self.auth_entering.peek() {
            self.auth_entering.set(false);
            self.replace_auth_key(String::new());
        } else {
            self.auth_open.set(false);
        }
    }

    pub(in crate::tui::sigil_view) fn replace_auth_key(&self, key: String) {
        self.auth_caret.set(key.len());
        self.auth_key.set(key);
    }
}
