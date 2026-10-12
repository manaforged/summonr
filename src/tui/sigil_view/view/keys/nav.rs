use super::*;

impl App {
    fn edge(&self, event: &mut KeyEvent, end: bool) {
        if self.overlay() == Some(Overlay::Usage) {
            return;
        }
        if self.pane_active().is_some() {
            event.stop_propagation();
            self.pane_edge(end);
        } else if self.overlay().is_none() && self.draft.peek().is_empty() {
            event.stop_propagation();
            self.history_edge(end);
        }
    }

    fn ctrl_edge(&self, event: &mut KeyEvent, end: bool) {
        if self.overlay().is_none() {
            event.stop_propagation();
            self.history_edge(end);
        }
    }

    fn page(&self, event: &mut KeyEvent, dir: isize) {
        if self.overlay() == Some(Overlay::Usage) {
            return;
        }
        event.stop_propagation();
        if self.overlay_move(8 * dir) {
            return;
        }
        if self.pane_active().is_some() {
            self.pane_page(dir);
        } else {
            self.page_history(dir as f32);
        }
    }

    fn arrow(&self, event: &mut KeyEvent, dir: isize) {
        if self.overlay_move(dir) || self.form_move(dir) {
            event.stop_propagation();
        } else if self.context.peek().is_some() {
            event.stop_propagation();
            self.context_move(dir);
        } else if self.pane_active().is_some() {
            event.stop_propagation();
            self.pane_move(dir);
        } else if dir < 0 && self.draft.peek().is_empty() && self.model.get().queued.1 > 0 {
            event.stop_propagation();
            self.dequeue();
        } else if self.suggesting() {
            event.stop_propagation();
            self.shift(dir);
        }
    }

    fn adjust(&self, event: &mut KeyEvent, dir: isize) {
        if self.overlay_adjust(dir) {
            event.stop_propagation();
        }
    }
}

pub(super) fn nav() -> Vec<(Shortcut, Handler)> {
    vec![
        (Key::Home.into(), |app, event| app.edge(event, false)),
        (Key::End.into(), |app, event| app.edge(event, true)),
        (Shortcut::ctrl(Key::Home), |app, event| {
            app.ctrl_edge(event, false)
        }),
        (Shortcut::ctrl(Key::End), |app, event| {
            app.ctrl_edge(event, true)
        }),
        (Key::PageUp.into(), |app, event| app.page(event, -1)),
        (Key::PageDown.into(), |app, event| app.page(event, 1)),
        (Key::ArrowUp.into(), |app, event| app.arrow(event, -1)),
        (Key::ArrowDown.into(), |app, event| app.arrow(event, 1)),
        (Key::ArrowLeft.into(), |app, event| app.adjust(event, -1)),
        (Key::ArrowRight.into(), |app, event| app.adjust(event, 1)),
        (LogicalKey::character(" ").into(), |app, event| {
            if !app.options_active() && app.form_toggle() {
                event.stop_propagation();
            }
        }),
    ]
}
