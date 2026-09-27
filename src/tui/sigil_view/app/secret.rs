use sigil::prelude::effect;

use super::App;
use crate::askpass::{Request, Secret};

impl App {
    pub(in crate::tui::sigil_view) fn set_secret(&self, state: Option<(String, usize)>) {
        self.secret_dialog.set(state.is_some());
        self.secret.set(state);
    }

    pub(in crate::tui::sigil_view) fn secret_open(&self, request: Request) {
        if let Some((stale, _)) = self.pending.borrow_mut().take() {
            stale.answer(None);
        }
        self.set_secret(Some((request.prompt.clone(), 0)));
        *self.pending.borrow_mut() = Some((request, Secret::new(String::new())));
    }

    pub(in crate::tui::sigil_view) fn secret_active(&self) -> bool {
        self.pending.borrow().is_some()
    }

    pub(in crate::tui::sigil_view) fn secret_push(&self, text: &str) {
        let mut pending = self.pending.borrow_mut();
        let Some((request, typed)) = pending.as_mut() else {
            return;
        };
        typed.push_str(text);
        self.set_secret(Some((request.prompt.clone(), typed.chars().count())));
    }

    pub(in crate::tui::sigil_view) fn secret_pop(&self) {
        let mut pending = self.pending.borrow_mut();
        let Some((request, typed)) = pending.as_mut() else {
            return;
        };
        typed.pop();
        self.set_secret(Some((request.prompt.clone(), typed.chars().count())));
    }

    pub(in crate::tui::sigil_view) fn secret_finish(&self, send: bool) {
        let taken = self.pending.borrow_mut().take();
        if let Some((request, typed)) = taken {
            request.answer((send && !typed.is_empty()).then_some(typed));
        }
        self.set_secret(None);
    }

    pub(in crate::tui::sigil_view) fn cancel_secret_on_close(&self) {
        let app = self.clone();
        effect(move || {
            if !app.secret_dialog.get() && app.secret_active() {
                app.secret_finish(false);
            }
        });
    }
}
