use sigil::prelude::{effect, interval};

use super::*;
use crate::scene;

fn sky_clock(app: &App) -> sigil::prelude::Signal<f64> {
    let seconds = sigil::prelude::signal(0.0_f64);
    let start = std::time::Instant::now();
    let app = app.clone();
    effect(move || {
        let delay = scene::next_change(&app, seconds.get());
        let wake = app.clone();
        interval(delay, move || {
            seconds.set(start.elapsed().as_secs_f64());
            wake.reveal();
        });
    });
    seconds
}

impl App {
    pub(crate) fn refresh(&self) {
        batch(|| {
            let model = self.model.get();
            if !model.entries.is_empty() {
                self.splash.set(false);
            }
            self.status.set(model.status_line(&self.info));
        });
    }

    pub(crate) fn refresh_status(&self) {
        self.status.set(self.model.get().status_line(&self.info));
    }

    pub(crate) fn resize(&self, cols: u16, rows: u16) {
        self.viewport.set((cols, rows));
    }

    pub(crate) fn animate(&self) {
        let app = self.clone();
        let busy = sigil::reactive::derived(move || {
            app.model
                .with(|model| super::super::chat::strip::working(model))
        });
        let app = self.clone();
        let splash = sigil::reactive::derived(move || {
            (app.splash.get() && app.model.with(|model| model.prefs.animations) && !app.covered())
                || !app.splash_revealed.get()
        });
        self.ticks_while(splash, busy);
        let app = self.clone();
        effect(move || {
            app.sky_clock.set(splash.get().then(|| sky_clock(&app)));
        });
        self.poll();
        self.clear_notices();
        super::files::expire(self.revert_armed);
        super::files::expire(self.worktree_discard);
    }

    fn ticks_while(
        &self,
        splash: sigil::prelude::Signal<bool>,
        busy: sigil::prelude::Signal<bool>,
    ) {
        let app = self.clone();
        effect(move || {
            let period = match (splash.get(), busy.get()) {
                (true, true) => Some(summonr_agent::limits::limits().animation()),
                (true, false) => None,
                (false, busy) => busy.then_some(summonr_agent::limits::limits().spinner()),
            };
            if let Some(period) = period {
                let tick = app.clone();
                interval(period, move || tick.tick());
            }
            if busy.get() {
                let status = app.clone();
                interval(
                    summonr_agent::limits::limits().status_refresh(),
                    move || status.refresh_status(),
                );
            }
        });
    }

    fn clear_notices(&self) {
        let app = self.clone();
        effect(move || {
            if app.notice.get().is_some() {
                let clear = app.clone();
                interval(summonr_agent::limits::limits().notice(), move || {
                    clear.notice.set(None)
                });
            }
        });
    }

    pub(crate) fn copy_notice(
        &self,
        text: &str,
        result: &sigil::AppResult<sigil::clipboard::Delivery>,
    ) {
        let lines = text.lines().count().max(1);
        let plural = if lines == 1 { "" } else { "s" };
        let (text, kind) = match result {
            Ok(sigil::clipboard::Delivery::Confirmed) => {
                (format!("copied {lines} line{plural}"), NoticeKind::Copied)
            }
            Ok(sigil::clipboard::Delivery::Requested) => (
                format!("sent {lines} line{plural} to the terminal clipboard"),
                NoticeKind::Copied,
            ),
            Err(error) => (format!("copy failed: {error}"), NoticeKind::Failed),
        };
        self.notice.set(Some(Notice { text, kind }));
    }

    fn poll(&self) {
        let app = self.clone();
        effect(move || {
            if app.layout.get().secondary.is_some_and(App::pane_polls) {
                let poll = app.clone();
                interval(summonr_agent::limits::limits().beat(), move || poll.load());
            }
        });
    }

    fn tick(&self) {
        self.ticks.update(|ticks| *ticks = ticks.saturating_add(1));
    }

    fn reveal(&self) {
        if !self.splash_revealed.peek()
            && self.splash_since.get().elapsed().as_millis()
                > u128::from(
                    summonr_agent::limits::limits()
                        .splash_reveal_ms
                        .max(scene::reveal_ms(self)),
                )
        {
            self.splash_revealed.set(true);
        }
    }

    pub(crate) fn persist(&self, prefs: &Prefs) {
        if let Err(error) = save(self.prefs_path.as_deref(), prefs) {
            self.push_entry(Entry::Error(error));
        }
    }
}
