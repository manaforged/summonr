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
            self.set_layout(Layout {
                secondary: Some(pane),
                ..layout
            });
            self.pane_focus.set(true);
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
            Some(Pane::Worktrees) => self.tree_rows().len(),
            Some(Pane::Jobs) => self.job_rows().len(),
            Some(Pane::Mcp) => self.mcp_rows().len(),
            Some(Pane::Peers) => self.model.get().peers.len(),
            Some(Pane::Diff) if self.diff_view.peek().is_some() => self.hunks().len(),
            Some(Pane::Diff) => self.diff_rows().len(),
            Some(Pane::Chat | Pane::Ledger | Pane::Files) | None => self.file_rows().len(),
        }
    }

    pub(in crate::tui::sigil_view) fn pane_collapse(&self) {
        if self.layout.peek().secondary == Some(Pane::View) {
            self.pane_back();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees)
            || self.layout.peek().secondary == Some(Pane::Peers)
        {
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Jobs) {
            self.job_open.set(None);
            self.refresh();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Mcp) {
            self.mcp_open.set(None);
            self.refresh();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            self.pane_back();
            return;
        }
        let rows = self.file_rows();
        let cursor = self.file_cursor.peek().min(rows.len().saturating_sub(1));
        let Some(row) = rows.get(cursor) else {
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
        if self.layout.peek().secondary == Some(Pane::View) {
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            self.tree_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Jobs) {
            self.job_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Mcp) {
            self.mcp_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Peers) {
            self.peer_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            if self.diff_view.peek().is_none() {
                self.diff_enter();
            }
            return;
        }
        let rows = self.file_rows();
        let cursor = self.file_cursor.peek().min(rows.len().saturating_sub(1));
        match rows.get(cursor) {
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
        if self.layout.peek().secondary == Some(Pane::View) {
            self.view_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            self.tree_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Jobs) {
            self.job_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Mcp) {
            self.mcp_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Peers) {
            self.peer_move(delta);
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            self.diff_move(delta);
            return;
        }
        let count = self.file_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.file_cursor.peek().min(count - 1) as isize;
        let next = (cursor + delta).clamp(0, count as isize - 1) as usize;
        self.file_cursor.set(next);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn pane_enter(&self) {
        if self.layout.peek().secondary == Some(Pane::View) {
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Worktrees) {
            self.tree_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Jobs) {
            self.job_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Mcp) {
            self.mcp_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Peers) {
            self.peer_enter();
            return;
        }
        if self.layout.peek().secondary == Some(Pane::Diff) {
            self.diff_enter();
            return;
        }
        let rows = self.file_rows();
        let Some(row) = rows.get(self.file_cursor.peek().min(rows.len().saturating_sub(1))) else {
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
