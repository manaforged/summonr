use crate::askpass::{Request, Secret};

use super::App;

impl App {
    pub(in super::super) fn secret_open(&self, request: Request) {
        if let Some((stale, _)) = self.pending.borrow_mut().take() {
            stale.answer(None);
        }
        self.secret.set(Some((request.prompt.clone(), 0)));
        *self.pending.borrow_mut() = Some((request, Secret::new(String::new())));
    }

    pub(in super::super) fn secret_active(&self) -> bool {
        self.pending.borrow().is_some()
    }

    pub(in super::super) fn secret_push(&self, text: &str) {
        let mut pending = self.pending.borrow_mut();
        let Some((request, typed)) = pending.as_mut() else {
            return;
        };
        typed.push_str(text);
        self.secret
            .set(Some((request.prompt.clone(), typed.chars().count())));
    }

    pub(in super::super) fn secret_pop(&self) {
        let mut pending = self.pending.borrow_mut();
        let Some((request, typed)) = pending.as_mut() else {
            return;
        };
        typed.pop();
        self.secret
            .set(Some((request.prompt.clone(), typed.chars().count())));
    }

    pub(in super::super) fn secret_finish(&self, send: bool) {
        let taken = self.pending.borrow_mut().take();
        if let Some((request, typed)) = taken {
            request.answer((send && !typed.is_empty()).then_some(typed));
        }
        self.secret.set(None);
    }
}
