use super::*;

const NO_PROVIDER_TERMINAL: &str = "no provider is set up; /login adds one";
const NO_PROVIDER_WINDOW: &str = "No provider is set up. Add a provider to start.";

fn no_provider() -> &'static str {
    if crate::tui::sigil_view::canvas::is_window() {
        NO_PROVIDER_WINDOW
    } else {
        NO_PROVIDER_TERMINAL
    }
}

impl App {
    pub(in crate::tui::sigil_view) fn open_providers(&self) {
        self.begin_auth(None);
    }

    pub(in crate::tui::sigil_view) fn ask_provider(&self) {
        self.open_providers();
        self.auth
            .step
            .set(Login::List(Note::plain(no_provider().to_string())));
    }

    pub(in crate::tui::sigil_view) fn unseated(&self) -> bool {
        crate::endpoint::resolve(
            Some(&self.model.peek().snapshot.endpoint),
            None,
            crate::endpoint::Need::Seat,
        )
        .is_err()
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
            self.push_entry(Entry::System(no_provider().to_string()));
            self.splash.set(false);
            self.refresh();
        }
        self.auth.focus.set(self.auth.focus.peek() + 1);
        self.auth.step.set(Login::List(Note::default()));
    }

    pub(in crate::tui::sigil_view) fn dismiss_auth(&self) {
        if self.auth.open.peek() && !self.auth_listing() {
            self.cancel_auth();
        }
        if self.auth.open.peek() {
            self.cancel_auth();
        }
    }
}
