use super::*;

impl App {
    pub(super) fn edge(&self, end: bool) -> bool {
        if self.overlay() == Some(Overlay::Usage) {
            return false;
        }
        if self.pane_active().is_some() {
            self.pane_edge(end);
        } else if self.overlay().is_none() && self.draft.peek().is_empty() {
            self.history_edge(end);
        } else {
            return false;
        }
        true
    }

    pub(super) fn ctrl_edge(&self, end: bool) -> bool {
        let free = self.overlay().is_none();
        if free {
            self.history_edge(end);
        }
        free
    }

    pub(super) fn page(&self, dir: isize) -> bool {
        if self.overlay() == Some(Overlay::Usage) {
            return false;
        }
        if self.overlay_move(8 * dir) {
            return true;
        }
        if self.pane_active().is_some() {
            self.pane_page(dir);
        } else {
            self.page_history(dir as f32);
        }
        true
    }

    pub(super) fn arrow(&self, dir: isize) -> bool {
        if self.overlay_move(dir) || self.form_move(dir) {
        } else if self.pane_active().is_some() {
            self.pane_move(dir);
        } else if dir < 0 && self.draft.peek().is_empty() && self.model.get().queued.1 > 0 {
            self.dequeue();
        } else if self.suggesting() {
            self.shift(dir);
        } else {
            return false;
        }
        true
    }
}
