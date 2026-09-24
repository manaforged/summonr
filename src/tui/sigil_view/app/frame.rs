use super::*;

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
            self.reflow();
        });
    }

    pub(in crate::tui::sigil_view) fn refresh_status(&self) {
        self.status.set(status(&self.model.get(), &self.info));
    }

    pub(in crate::tui::sigil_view) fn resize(&self, cols: u16, rows: u16) {
        batch(|| {
            self.viewport.set((cols, rows));
            self.constrain();
            self.reveal(&self.draft.peek());
            self.reflow();
        });
    }

    pub(in crate::tui::sigil_view) fn tick(&self) {
        let busy = self.model.peek().busy_since.is_some();
        let ticks = self.ticks.peek().saturating_add(1);
        let splash = self.splash.peek() && self.model.get().prefs.animations;
        if busy || splash || !self.splash_revealed.peek() {
            self.ticks.set(ticks);
        }
        if !self.splash_revealed.peek() && ticks.saturating_mul(40) > 760 {
            self.splash_revealed.set(true);
        }
    }

    pub(in crate::tui::sigil_view) fn persist(&self, prefs: &Prefs) {
        if let Err(error) = save(self.prefs_path.as_deref(), prefs) {
            self.push_entry(Entry::Error(error));
        }
    }
}
