pub(in crate::tui::sigil_view) use super::note::Note;
use rank::best_match;
pub(in crate::tui::sigil_view) use rank::matching;

mod rank;
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Login {
    List(Note),
    Key(String),
    Checking,
    SignIn {
        text: String,
        url: String,
        code: String,
        asking: bool,
        note: String,
    },
    Confirm,
}

impl Login {
    fn signing() -> Self {
        Self::SignIn {
            text: String::new(),
            url: String::new(),
            code: String::new(),
            asking: false,
            note: String::new(),
        }
    }
}

fn providers() -> Vec<crate::endpoint::AuthInfo> {
    let mut choices: Vec<_> = crate::endpoint::names()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|name| crate::endpoint::auth_info(&name).ok())
        .collect();
    choices.sort_by_key(|choice| (section(choice), choice.catalog_rank(), choice.name.clone()));
    choices
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui::sigil_view) enum Tier {
    Ready,
    Unset,
}

impl Tier {
    pub(in crate::tui::sigil_view) const fn title(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Unset => "not set up",
        }
    }
}

pub(in crate::tui::sigil_view) fn section(choice: &crate::endpoint::AuthInfo) -> Tier {
    if choice.unmodeled() {
        return Tier::Unset;
    }
    if choice.configured || choice.keyless() {
        return Tier::Ready;
    }
    Tier::Unset
}

impl App {
    pub(in crate::tui::sigil_view) fn begin_auth(&self, focus: Option<&str>) {
        if let Some(name) = focus
            && let Err(error) = crate::endpoint::select(name)
        {
            self.push_entry(Entry::Error(error.to_string()));
        }
        let choices = providers();
        let current = self.model.get().snapshot.endpoint.clone();
        let focus = focus.map(|name| {
            let provider = crate::endpoint::auth_info(name).map(|info| info.provider);
            choices
                .iter()
                .find(|choice| {
                    provider
                        .as_deref()
                        .is_ok_and(|named| choice.provider == named)
                })
                .map_or(name, |choice| choice.name.as_str())
                .to_string()
        });
        let focus = focus.as_deref();
        if let Some(name) = focus
            && !choices.iter().any(|choice| choice.name == name)
        {
            self.push_entry(Entry::Error(format!(
                "provider `{name}` is unavailable; /login chooses a provider"
            )));
            self.splash.set(false);
            self.refresh();
            return;
        }
        let asked = focus.is_some();
        let focus = focus.unwrap_or(&current);
        let selected = choices
            .iter()
            .position(|choice| choice.name == focus)
            .or_else(|| choices.iter().position(|choice| choice.name == current))
            .unwrap_or(0);
        let named = asked && choices.get(selected).is_some_and(|row| !row.dual());
        self.auth.choices.set(choices);
        self.auth.filter.set(String::new());
        self.auth.seat.set(None);
        self.auth.focus.set(self.auth.focus.peek() + 1);
        self.auth.sel.set(selected);
        self.auth.step.set(Login::List(Note::default()));
        self.replace_auth_key(String::new());
        self.auth.open.set(true);
        if named {
            self.commit_auth();
        } else if !self.picker.open.peek() {
            self.list_models(None);
        }
    }

    pub(in crate::tui::sigil_view) fn move_auth(&self, delta: isize) {
        if !self.auth_listing() || self.picker.open.peek() {
            return;
        }
        let last = self.filtered_auth().len().saturating_sub(1);
        self.auth
            .sel
            .set(self.auth.sel.peek().saturating_add_signed(delta).min(last));
    }

    pub(in crate::tui::sigil_view) fn auth_choice(&self) -> Option<crate::endpoint::AuthInfo> {
        let choices = self.filtered_auth();
        choices
            .get(self.auth.sel.peek().min(choices.len().saturating_sub(1)))
            .cloned()
    }

    pub(in crate::tui::sigil_view) fn commit_auth(&self) {
        let Some(choice) = self.auth_choice() else {
            return;
        };
        let name = choice.name.clone();
        match self.auth.step.peek() {
            Login::List(_) if choice.login.signs_in() => self.sign_in(name),
            Login::List(_) => self.key_entry(),
            Login::Key(_) => self.submit_key(name),
            Login::SignIn { asking: true, .. } => self.submit_code(),
            Login::SignIn { note, .. } if !note.is_empty() => self.sign_in(name),
            Login::Confirm => {
                self.auth.step.set(Login::Checking);
                drop(self.cmd.send(Cmd::Logout(name)));
            }
            Login::SignIn { .. } | Login::Checking => {}
        }
    }

    pub(in crate::tui::sigil_view) fn filtered_auth(&self) -> Vec<crate::endpoint::AuthInfo> {
        matching(self.auth.choices.peek(), &self.auth.filter.peek())
    }

    pub(in crate::tui::sigil_view) fn auth_listing(&self) -> bool {
        matches!(self.auth.step.peek(), Login::List(_))
    }

    pub(in crate::tui::sigil_view) fn pick_auth(&self, index: usize) {
        if self.auth_listing() && !self.picker.open.peek() {
            self.auth.sel.set(index);
            self.commit_auth();
        }
    }

    pub(in crate::tui::sigil_view) fn replace_auth_filter(&self, filter: String) {
        if self.auth_listing() {
            let best = best_match(&matching(self.auth.choices.peek(), &filter), &filter);
            self.auth.filter.set(filter);
            self.auth.sel.set(best);
        }
    }

    pub(in crate::tui::sigil_view) fn toggle_route(&self, toggle: crate::endpoint::Toggle) {
        let Some(choice) = self.auth_choice() else {
            return;
        };
        if !matches!(self.auth.step.peek(), Login::List(_)) {
            return;
        }
        match crate::endpoint::toggle(&choice, toggle) {
            Ok(Some(_)) => {
                self.auth.choices.set(providers());
                let rows = self.filtered_auth();
                let selected = rows
                    .iter()
                    .position(|row| row.provider == choice.provider)
                    .unwrap_or(0);
                self.auth.sel.set(selected);
                self.list_models(None);
            }
            Ok(None) => {}
            Err(error) => self
                .auth
                .step
                .set(Login::List(Note::failed(error.to_string()))),
        }
    }

    pub(in crate::tui::sigil_view) fn key_entry(&self) {
        let Some(choice) = self.auth_choice() else {
            return;
        };
        let takes = choice.key || choice.keyless();
        if !matches!(self.auth.step.peek(), Login::List(_)) || !takes || self.env_note(&choice) {
            return;
        }
        self.auth.step.set(Login::Key(String::new()));
        self.auth.focus.set(self.auth.focus.peek() + 1);
        self.replace_auth_key(String::new());
    }

    fn submit_key(&self, name: String) {
        let key = self.auth.key.peek();
        if key.trim().is_empty() {
            return;
        }
        self.auth.step.set(Login::Checking);
        drop(self.cmd.send(Cmd::SetKey(name, key)));
    }

    fn submit_code(&self) {
        let code = self.auth.key.peek();
        if code.trim().is_empty() {
            return;
        }
        self.replace_auth_key(String::new());
        self.auth.step.set(Login::signing());
        drop(self.cmd.send(Cmd::LoginAnswer(code)));
    }

    fn sign_in(&self, name: String) {
        self.replace_auth_key(String::new());
        self.auth.step.set(Login::signing());
        drop(self.cmd.send(Cmd::Login(name)));
    }

    pub(in crate::tui::sigil_view) fn confirm_logout(&self) {
        let Some(choice) = self.auth_choice() else {
            return;
        };
        if matches!(self.auth.step.peek(), Login::List(_))
            && !self.env_note(&choice)
            && choice.stored()
        {
            self.auth.step.set(Login::Confirm);
        }
    }

    pub(in crate::tui::sigil_view) fn unseated(&self) -> bool {
        crate::endpoint::resolve(
            Some(&self.model.peek().snapshot.endpoint),
            None,
            crate::endpoint::Need::Seat,
        )
        .is_err()
    }

    fn env_note(&self, choice: &crate::endpoint::AuthInfo) -> bool {
        let Some(env) = choice.env else {
            return false;
        };
        self.auth.step.set(Login::List(Note::plain(format!(
            "key comes from ${env}; unset it to sign out"
        ))));
        self.auth.focus.set(self.auth.focus.peek() + 1);
        true
    }

    pub(in crate::tui::sigil_view) fn cancel_auth(&self) {
        let step = self.auth.step.peek();
        let waiting = matches!(step, Login::Checking)
            || matches!(step, Login::SignIn { ref note, .. } if note.is_empty());
        if waiting {
            drop(self.cmd.send(Cmd::Cancel));
        }
        self.replace_auth_key(String::new());
        self.auth.seat.set(None);
        if matches!(step, Login::List(_)) || self.picker.open.peek() {
            self.auth.open.set(false);
        }
        if matches!(step, Login::List(_)) && !self.picker.open.peek() && self.unseated() {
            self.push_entry(Entry::System(
                "no provider is set up; /login adds one".to_string(),
            ));
            self.splash.set(false);
            self.refresh();
        }
        self.auth.focus.set(self.auth.focus.peek() + 1);
        self.auth.step.set(Login::List(Note::default()));
    }

    pub(in crate::tui::sigil_view) fn auth_progress(
        &self,
        text: String,
        url: String,
        code: String,
        asking: bool,
    ) {
        let Login::SignIn {
            text: last,
            url: shown,
            code: typed,
            ..
        } = self.auth.step.peek()
        else {
            return;
        };
        if asking {
            self.auth.focus.set(self.auth.focus.peek() + 1);
        }
        self.auth.step.set(Login::SignIn {
            text: if text.is_empty() { last } else { text },
            url: if url.is_empty() { shown } else { url },
            code: if code.is_empty() { typed } else { code },
            asking,
            note: String::new(),
        });
    }

    pub(in crate::tui::sigil_view) fn auth_done(
        &self,
        name: &str,
        result: std::result::Result<String, String>,
        rejected: bool,
    ) {
        let mut groups = self.picker.groups.peek();
        match result {
            Ok(text) => {
                groups.remove(name);
                self.picker.groups.set(groups);
                self.replace_auth_key(String::new());
                if self.picker.open.peek() {
                    self.push_entry(Entry::System(text.clone()));
                    self.auth.open.set(false);
                    match self.auth.seat.peek().filter(|(owner, _)| owner == name) {
                        Some((owner, model)) => {
                            self.cancel_model_edit();
                            drop(self.cmd.send(Cmd::SetSeat(owner, model)));
                        }
                        None => self.refresh_models(),
                    }
                    return;
                }
                self.auth.choices.set(providers());
                let found = self
                    .filtered_auth()
                    .iter()
                    .position(|choice| choice.name == name);
                self.auth.sel.set(found.unwrap_or_default());
                self.auth.step.set(Login::List(Note::plain(text)));
                self.auth.focus.set(self.auth.focus.peek() + 1);
            }
            Err(text) if text.is_empty() => {}
            Err(text) if !self.auth.open.peek() => self.push_entry(Entry::Error(text)),
            Err(text) => {
                if rejected {
                    groups.insert(name.to_string(), State::KeyRejected);
                    self.picker.groups.set(groups);
                }
                let step = match self.auth.step.peek() {
                    Login::SignIn {
                        text: shown,
                        url,
                        code,
                        ..
                    } => Login::SignIn {
                        text: shown,
                        url,
                        code,
                        asking: false,
                        note: text,
                    },
                    Login::Checking if !self.auth.key.peek().is_empty() => Login::Key(text),
                    Login::List(_) | Login::Key(_) | Login::Checking | Login::Confirm => {
                        Login::List(Note::failed(text))
                    }
                };
                self.auth.step.set(step);
            }
        }
    }

    pub(in crate::tui::sigil_view) fn replace_auth_key(&self, key: String) {
        self.auth.caret.set(key.len());
        self.auth.key.set(key);
    }
}
