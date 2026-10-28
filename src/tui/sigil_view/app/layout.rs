use super::*;
use crate::tui::sigil_view::ui::{Hints, Key};

impl App {
    pub(in crate::tui::sigil_view) fn set_layout(&self, layout: Layout) {
        if self.view_edit.peek() && layout.secondary != Some(Pane::View) {
            if self.view_text.peek() != self.view_saved.peek() {
                let keys = Hints::new()
                    .key(Key::Fixed("ctrl+s"), "save")
                    .key(Key::Esc, "leave")
                    .text(&self.model.peek().prefs.hotkeys);
                self.push_entry(Entry::Error(format!(
                    "unsaved edits in the view pane: {keys}"
                )));
                self.refresh();
                return;
            }
            self.view_edit.set(false);
        }
        let opened = self.layout.peek().secondary != layout.secondary;
        if layout.secondary.is_none() {
            self.pane_focus.set(false);
        }
        self.layout.set(layout);
        if layout.secondary.is_some() {
            self.splash.set(false);
        }
        if opened {
            self.load();
        }
        if opened && layout.secondary.is_some() && layout.split(self.viewport.peek().0).1 == 0 {
            self.push_entry(Entry::System(format!(
                "widen to {} columns for panes",
                Layout::MIN_COLS
            )));
        }
        self.model_mut(|model| model.prefs.layout = layout.text());
        let prefs = self.model.peek().prefs.clone();
        self.persist(&prefs);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn toggle_pane(&self, pane: Pane) {
        if pane == Pane::Diff {
            self.compare(None);
        }
        let layout = self.layout.peek();
        if layout.secondary == Some(pane) {
            self.last_pane.set(Some(pane));
            self.set_layout(Layout {
                secondary: None,
                ..layout
            });
        } else {
            let share = if pane == Pane::Diff {
                layout.share.max(Layout::REVIEW_SHARE)
            } else {
                layout.share
            };
            self.set_layout(Layout {
                secondary: Some(pane),
                share,
            });
            if !crate::tui::sigil_view::canvas::is_window() {
                self.pane_focus.set(true);
            }
        }
    }

    pub(in crate::tui::sigil_view) fn toggle_side(&self) {
        let layout = self.layout.peek();
        match layout.secondary {
            Some(pane) => self.toggle_pane(pane),
            None => {
                let pane = self.last_pane.peek().unwrap_or(Pane::Files);
                self.toggle_pane(pane);
            }
        }
    }

    pub(in crate::tui::sigil_view) fn focus_pane(&self, on: bool) {
        let has_pane = self.layout.peek().secondary.is_some();
        self.pane_focus.set(on && has_pane);
        if !self.pane_focus.peek() {
            self.focus_composer();
        }
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn pane_active(&self) -> Option<Pane> {
        if self.view_edit.peek()
            || self.note_target.peek().is_some()
            || self.layout.peek().split(self.viewport.peek().0).1 == 0
        {
            return None;
        }
        self.pane_focus
            .peek()
            .then(|| self.layout.peek().secondary)
            .flatten()
    }

    pub(in crate::tui::sigil_view) fn pane_back(&self) -> bool {
        let layout = self.layout.peek();
        if layout.secondary == Some(Pane::View) {
            let back = self.view_from.peek().unwrap_or(Pane::Files);
            self.set_layout(Layout {
                secondary: Some(back),
                ..layout
            });
            self.pane_focus.set(true);
            self.refresh();
            return true;
        }
        if layout.secondary != Some(Pane::Diff) {
            return false;
        }
        if self.diff_view.peek().is_some() {
            self.note_target.set(None);
            self.hunk_list.select(0);
            self.diff_view.set(None);
            self.refresh();
            return true;
        }
        if self.diff_root.peek().is_some() {
            self.compare(None);
            self.set_layout(Layout {
                secondary: Some(Pane::Worktrees),
                ..layout
            });
            self.pane_focus.set(true);
            self.refresh();
            return true;
        }
        false
    }

    pub(in crate::tui::sigil_view) fn pane_dismiss(&self) -> bool {
        let layout = self.layout.peek();
        let Some(pane) = layout.secondary else {
            return false;
        };
        if !self.draft.peek().is_empty() || self.overlay().is_some() {
            return false;
        }
        self.last_pane.set(Some(pane));
        self.set_layout(Layout {
            secondary: None,
            ..layout
        });
        self.refresh();
        true
    }

    pub(in crate::tui::sigil_view) fn nudge_split(&self, delta: i16) {
        let layout = self.layout.peek();
        if layout.secondary.is_some() {
            self.set_layout(layout.nudge(delta));
        }
    }

    pub(in crate::tui::sigil_view) fn pane_command(&self, argument: Option<&str>) {
        let layout = self.layout.peek();
        match argument {
            None => {
                let names: Vec<&str> = Pane::SECONDARY.iter().map(|pane| pane.name()).collect();
                self.push_entry(Entry::System(format!(
                    "layout: {} · /pane {} · off · focus · run · review",
                    layout.text(),
                    names.join(" · ")
                )));
                self.refresh();
            }
            Some(name) => match Target::parse(name) {
                Some(Target::Off) => self.set_layout(Layout {
                    secondary: None,
                    ..layout
                }),
                Some(Target::Pane(pane)) => self.set_layout(Layout {
                    secondary: Some(pane),
                    ..layout
                }),
                Some(Target::Layout(preset)) => self.set_layout(preset),
                None => {
                    self.push_entry(Entry::Error(format!(
                        "no pane or layout `{name}`: files, ledger, diff, worktrees, jobs, mcp, peers, off, focus, run, review, or `chat | ledger 40%`"
                    )));
                    self.refresh();
                }
            },
        }
    }
}
