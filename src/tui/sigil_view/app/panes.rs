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

    pub(in crate::tui::sigil_view) fn load(&self) {
        match self.layout.peek().secondary {
            Some(Pane::Files) => drop(self.cmd.send(Cmd::ListFiles)),
            Some(Pane::Diff) => drop(self.cmd.send(Cmd::Diff(self.root()))),
            Some(Pane::Worktrees) => drop(self.cmd.send(Cmd::Trees)),
            Some(Pane::Jobs) => drop(self.cmd.send(Cmd::Jobs)),
            Some(Pane::Mcp) => drop(self.cmd.send(Cmd::Mcp)),
            Some(Pane::Chat | Pane::Ledger | Pane::Peers | Pane::View) | None => {}
        }
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

    pub(in crate::tui::sigil_view) fn pane_page(&self, delta: isize) {
        let rows = isize::from(
            i16::try_from(self.viewport.peek().1.saturating_sub(4).max(1)).unwrap_or(1),
        );
        self.pane_move(delta * rows);
    }

    pub(in crate::tui::sigil_view) fn pane_edge(&self, end: bool) {
        let last = self.pane_len().saturating_sub(1) as isize;
        self.pane_move(if end { last } else { -last });
    }

    fn pane_len(&self) -> usize {
        match self.layout.peek().secondary {
            Some(Pane::View) => self.view_lines(),
            Some(Pane::Diff) if self.diff_view.peek().is_some() => self.hunks().len(),
            other => self.pane_rows(other.unwrap_or(Pane::Files)).len(),
        }
    }

    pub(in crate::tui::sigil_view) fn pane_cursor(&self, pane: Pane) -> &Signal<usize> {
        match pane {
            Pane::Diff => &self.diff_cursor,
            Pane::Worktrees => &self.tree_cursor,
            Pane::Jobs => &self.job_cursor,
            Pane::Mcp => &self.mcp_cursor,
            Pane::Peers => &self.peer_cursor,
            Pane::Chat | Pane::Ledger | Pane::Files | Pane::View => &self.file_cursor,
        }
    }

    pub(in crate::tui::sigil_view) fn pane_rows(&self, pane: Pane) -> Vec<FileRow> {
        match pane {
            Pane::Diff => self.diff_rows(),
            Pane::Worktrees => self.tree_rows(),
            Pane::Jobs => self.job_rows(),
            Pane::Mcp => self.mcp_rows(),
            Pane::Peers => self.peer_rows(),
            Pane::Chat | Pane::Ledger | Pane::Files | Pane::View => self.file_rows(),
        }
    }

    pub(in crate::tui::sigil_view) fn pane_collapse(&self) {
        match self.layout.peek().secondary {
            Some(Pane::View | Pane::Diff) => {
                self.pane_back();
                return;
            }
            Some(Pane::Worktrees | Pane::Peers) => return,
            Some(Pane::Jobs) => {
                self.job_open.set(None);
                return self.refresh();
            }
            Some(Pane::Mcp) => {
                self.mcp_open.set(None);
                return self.refresh();
            }
            None | Some(Pane::Chat | Pane::Ledger | Pane::Files) => {}
        }
        let rows = self.file_rows();
        let Some(row) = rows.get(files::pick(&self.file_cursor, rows.len())) else {
            return;
        };
        if row.dir && row.open {
            self.pane_enter();
            return;
        }
        if let Some((parent, _)) = row.path.rsplit_once('/')
            && let Some(index) = rows.iter().position(|candidate| candidate.path == parent)
        {
            self.file_cursor.set(index);
            self.refresh();
        }
    }

    pub(in crate::tui::sigil_view) fn pane_expand(&self) {
        match self.layout.peek().secondary {
            Some(Pane::View) => return,
            Some(Pane::Diff) => {
                if self.diff_view.peek().is_none() {
                    self.diff_enter();
                }
                return;
            }
            Some(Pane::Worktrees) => return self.tree_enter(),
            Some(Pane::Jobs) => return self.job_enter(),
            Some(Pane::Mcp) => return self.mcp_enter(),
            Some(Pane::Peers) => return self.peer_enter(),
            None | Some(Pane::Chat | Pane::Ledger | Pane::Files) => {}
        }
        let rows = self.file_rows();
        match rows.get(files::pick(&self.file_cursor, rows.len())) {
            Some(row) if row.dir => {
                if !row.open {
                    self.pane_enter();
                }
            }
            Some(row) => self.view_open(row.path.clone()),
            None => {}
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

    pub(in crate::tui::sigil_view) fn pane_move(&self, delta: isize) {
        match self.layout.peek().secondary {
            Some(Pane::View) => self.view_move(delta),
            Some(Pane::Diff) => self.diff_move(delta),
            other => {
                let pane = other.unwrap_or(Pane::Files);
                files::shift(self.pane_cursor(pane), delta, self.pane_rows(pane).len());
                self.refresh();
            }
        }
    }

    pub(in crate::tui::sigil_view) fn pane_enter(&self) {
        match self.layout.peek().secondary {
            Some(Pane::View) => return,
            Some(Pane::Diff) => return self.diff_enter(),
            Some(Pane::Worktrees) => return self.tree_enter(),
            Some(Pane::Jobs) => return self.job_enter(),
            Some(Pane::Mcp) => return self.mcp_enter(),
            Some(Pane::Peers) => return self.peer_enter(),
            None | Some(Pane::Chat | Pane::Ledger | Pane::Files) => {}
        }
        let rows = self.file_rows();
        let Some(row) = rows.get(files::pick(&self.file_cursor, rows.len())) else {
            return;
        };
        if row.dir {
            let mut expanded = self.expanded.peek();
            match expanded.iter().position(|dir| *dir == row.path) {
                Some(index) => drop(expanded.remove(index)),
                None => expanded.push(row.path.clone()),
            }
            self.expanded.set(expanded);
        } else {
            self.mention(&row.path);
        }
        self.refresh();
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
            self.hunk_cursor.set(0);
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
            Some("off" | "close") => self.set_layout(Layout {
                secondary: None,
                ..layout
            }),
            Some(name) => match (Pane::parse(name), Layout::parse(name)) {
                (Some(pane), _) if pane != Pane::Chat => self.set_layout(Layout {
                    secondary: Some(pane),
                    ..layout
                }),
                (_, Some(preset)) => self.set_layout(preset),
                (Some(_) | None, None) => {
                    self.push_entry(Entry::Error(format!(
                        "no pane or layout `{name}`: files, ledger, diff, worktrees, jobs, mcp, peers, off, focus, run, review, or `chat | ledger 40%`"
                    )));
                    self.refresh();
                }
            },
        }
    }
}
