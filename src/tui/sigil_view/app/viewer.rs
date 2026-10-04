use super::*;
use crate::tui::sigil_view::ui::{Hints, Key};

impl App {
    pub(in crate::tui::sigil_view) fn view_open(&self, path: String) {
        let layout = self.layout.peek();
        if layout.secondary != Some(Pane::View) {
            self.view_from.set(layout.secondary);
            self.set_layout(Layout {
                secondary: Some(Pane::View),
                ..layout
            });
        }
        self.view_scroll.set(0);
        self.pane_focus.set(true);
        drop(self.cmd.send(Cmd::View(self.root(), path)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn edit_start(&self) {
        let text = self.model.get().view.as_ref().map(|view| view.text.clone());
        let Some(text) = text else {
            return;
        };
        self.view_saved.set(text.clone());
        self.view_text.set(text);
        self.view_caret.set(0);
        self.discard_armed.set(false);
        self.view_edit.set(true);
        self.pane_focus.set(true);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn edit_stop(&self) -> bool {
        if !self.view_edit.peek() {
            return false;
        }
        if self.view_text.peek() != self.view_saved.peek() && !self.discard_armed.peek() {
            self.discard_armed.set(true);
            let keys = Hints::new()
                .key(Key::Fixed("ctrl+s"), "save")
                .key(Key::Esc, "again discard")
                .text(&self.model.peek().prefs.hotkeys);
            self.push_entry(Entry::System(format!("unsaved edits — {keys}")));
            self.refresh();
            return true;
        }
        self.view_edit.set(false);
        self.discard_armed.set(false);
        self.refresh();
        true
    }

    pub(in crate::tui::sigil_view) fn edit_save(&self) {
        let path = self.model.get().view.as_ref().map(|view| view.file.clone());
        let Some(path) = path else {
            return;
        };
        let text = self.view_text.peek();
        self.discard_armed.set(false);
        drop(self.cmd.send(Cmd::Save(path, text)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn view_external(&self) {
        if let Some(path) = self.model.get().view.as_ref().map(|view| view.file.clone()) {
            self.open_file.set(Some(path));
        }
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn view_lines(&self) -> usize {
        self.model
            .get()
            .view
            .as_ref()
            .map_or(0, |view| view.text.lines().count())
    }

    pub(in crate::tui::sigil_view) fn view_move(&self, delta: isize) {
        let last = self.view_lines().saturating_sub(1) as isize;
        let scroll = (self.view_scroll.peek() as isize + delta).clamp(0, last);
        self.view_scroll.set(scroll as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn view_command(&self, argument: Option<&str>) {
        match argument {
            Some(path) => self.view_open(path.trim_start_matches('@').to_string()),
            None => {
                let path = self.model.get().view.as_ref().map(|view| view.path.clone());
                match path {
                    Some(path) => self.view_open(path),
                    None => {
                        self.push_entry(Entry::Error("usage: /view PATH".to_string()));
                        self.refresh();
                    }
                }
            }
        }
    }
}
