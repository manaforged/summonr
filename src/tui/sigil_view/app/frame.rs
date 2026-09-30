use sigil::prelude::{effect, interval};

use super::*;
use crate::limits::{ANIMATION, BEAT, SPINNER, SPLASH_REVEAL_MS, STATUS_REFRESH};

impl App {
    pub(in crate::tui::sigil_view) fn refresh(&self) {
        batch(|| {
            let model = self.model.get();
            if !model.entries.is_empty() {
                self.splash.set(false);
            }
            self.header.set(header(&model));
            self.status.set(status(&model, &self.info));
            self.settings
                .set(settings_text(&model.snapshot, &model.prefs));
            self.facts.set(facts(&model, &self.info));
            self.theme_name.set(model.prefs.theme.clone());
        });
    }

    pub(in crate::tui::sigil_view) fn refresh_status(&self) {
        self.status.set(status(&self.model.get(), &self.info));
    }

    pub(in crate::tui::sigil_view) fn resize(&self, cols: u16, rows: u16) {
        batch(|| {
            self.viewport.set((cols, rows));
            self.reveal(&self.draft.peek());
        });
    }

    pub(in crate::tui::sigil_view) fn animate(&self) {
        let app = self.clone();
        let busy =
            sigil::reactive::derived(move || app.model.with(|model| model.busy_since.is_some()));
        let app = self.clone();
        let splash = sigil::reactive::derived(move || {
            (app.splash.get() && app.model.with(|model| model.prefs.animations))
                || !app.splash_revealed.get()
        });
        let app = self.clone();
        effect(move || {
            let period = if splash.get() {
                Some(ANIMATION)
            } else {
                busy.get().then_some(SPINNER)
            };
            if let Some(period) = period {
                let tick = app.clone();
                interval(period, move || tick.tick());
            }
            if busy.get() {
                let status = app.clone();
                interval(STATUS_REFRESH, move || status.refresh_status());
            }
        });
        self.poll();
    }

    fn poll(&self) {
        let app = self.clone();
        effect(move || {
            let command: Option<fn() -> Cmd> = match app.layout.get().secondary {
                Some(super::Pane::Jobs) => Some(|| Cmd::Jobs),
                Some(super::Pane::Mcp) => Some(|| Cmd::Mcp),
                Some(
                    super::Pane::Chat
                    | super::Pane::Ledger
                    | super::Pane::Files
                    | super::Pane::Diff
                    | super::Pane::Worktrees
                    | super::Pane::Peers
                    | super::Pane::View,
                )
                | None => None,
            };
            if let Some(command) = command {
                let poll = app.clone();
                interval(BEAT, move || drop(poll.cmd.send(command())));
            }
        });
    }

    fn tick(&self) {
        self.ticks.update(|ticks| *ticks = ticks.saturating_add(1));
        if !self.splash_revealed.peek()
            && self.splash_since.get().elapsed().as_millis() > SPLASH_REVEAL_MS
        {
            self.splash_revealed.set(true);
        }
    }

    pub(in crate::tui::sigil_view) fn persist(&self, prefs: &Prefs) {
        if let Err(error) = save(self.prefs_path.as_deref(), prefs) {
            self.push_entry(Entry::Error(error));
        }
    }
}
