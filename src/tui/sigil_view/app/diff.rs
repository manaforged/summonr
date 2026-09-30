use super::*;

impl App {
    pub(in crate::tui::sigil_view) fn diff_stats(&self) -> Vec<crate::diff::Stat> {
        let model = self.model.get();
        let recent = self.diff_recent.peek();
        model
            .diff
            .files
            .iter()
            .filter(|stat| !recent || model.recent.contains(&stat.path))
            .cloned()
            .collect()
    }

    pub(in crate::tui::sigil_view) fn toggle_recent(&self) {
        self.diff_recent.set(!self.diff_recent.peek());
        self.diff_cursor.set(0);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn copy_patch(&self) {
        let viewed = self.diff_view.peek();
        let result = self
            .model
            .get()
            .patch
            .as_ref()
            .filter(|(path, _)| viewed.as_deref() == Some(path.as_str()))
            .map(|(_, result)| result.clone());
        match result {
            Some(result) => {
                self.copy_pending.set(None);
                self.patch_copy(result);
            }
            None => {
                let path = viewed.unwrap_or_default();
                let root = self.root();
                self.copy_pending.set(Some((root.clone(), path.clone())));
                drop(self.cmd.send(Cmd::Patch(root, path)));
            }
        }
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn patch_copy(&self, result: Result<String, String>) {
        match result {
            Ok(text) if !text.is_empty() => self.copy.set(Some(text)),
            Ok(_) => self.push_entry(Entry::System(
                "there are no patch changes to copy".to_string(),
            )),
            Err(error) => self.push_entry(Entry::Error(format!("cannot copy patch: {error}"))),
        }
    }

    pub(in crate::tui::sigil_view) fn copy_chat(&self, scope: Option<&str>) {
        let text = match scope {
            None => self
                .model
                .get()
                .entries
                .iter()
                .rev()
                .find_map(|entry| match entry {
                    Entry::Assistant { text, .. } if !text.trim().is_empty() => Some(text.clone()),
                    _ => None,
                }),
            Some("all") => Some(self.model.get().transcript()),
            Some(_) => {
                self.push_entry(Entry::System("usage: /copy [all]".to_string()));
                self.refresh();
                return;
            }
        };
        match text.filter(|text| !text.trim().is_empty()) {
            Some(text) => self.copy.set(Some(text)),
            None => self.push_entry(Entry::System("there is no reply to copy".to_string())),
        }
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn copied(
        &self,
        text: &str,
        status: sigil::clipboard::Delivery,
    ) {
        let lines = text.lines().count();
        let plural = if lines == 1 { "" } else { "s" };
        let notice = match status {
            sigil::clipboard::Delivery::Confirmed => {
                format!("copied {lines} line{plural} to the clipboard")
            }
            sigil::clipboard::Delivery::Requested => format!(
                "copy requested for {lines} line{plural}; terminal acceptance is unconfirmed"
            ),
        };
        self.push_entry(Entry::System(notice));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn diff_rows(&self) -> Vec<FileRow> {
        self.diff_stats()
            .iter()
            .map(|stat| FileRow {
                path: stat.path.clone(),
                name: stat.path.clone(),
                depth: 0,
                dir: false,
                open: false,
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn diff_move(&self, delta: isize) {
        if self.diff_view.peek().is_some() {
            return self.hunk_move(delta);
        }
        let count = self.diff_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.diff_cursor.peek().min(count - 1) as isize;
        self.diff_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn diff_enter(&self) {
        if self.diff_view.peek().is_some() {
            return self.hunk_accept();
        }
        if self.pane_back() {
            return;
        }
        let rows = self.diff_rows();
        let cursor = self.diff_cursor.peek().min(rows.len().saturating_sub(1));
        if let Some(row) = rows.get(cursor) {
            self.diff_open(row.path.clone());
        }
    }

    pub(in crate::tui::sigil_view) fn hunks(&self) -> Vec<crate::hunks::Hunk> {
        let Some(path) = self.diff_view.peek() else {
            return Vec::new();
        };
        let model = self.model.peek();
        let Some(Ok(patch)) = model
            .patch
            .as_ref()
            .filter(|(seen, _)| *seen == path)
            .map(|(_, result)| result)
        else {
            return Vec::new();
        };
        crate::hunks::split(&path, patch)
    }

    pub(in crate::tui::sigil_view) fn hunk_note(
        &self,
        path: &str,
        hunk: &str,
    ) -> Option<crate::hunks::Note> {
        let model = self.model.peek();
        let (root, marks) = model.marks.as_ref()?;
        if *root != self.root() {
            return None;
        }
        marks.get(path)?.get(hunk).cloned()
    }

    pub(in crate::tui::sigil_view) fn hunk_move(&self, delta: isize) {
        let count = self.hunks().len();
        if count == 0 {
            return;
        }
        let cursor = self.hunk_cursor.peek().min(count - 1) as isize;
        self.hunk_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn hunk_accept(&self) {
        let Some(path) = self.diff_view.peek() else {
            return;
        };
        let hunks = self.hunks();
        let Some(hunk) = hunks.get(self.hunk_cursor.peek().min(hunks.len().saturating_sub(1)))
        else {
            return;
        };
        self.hunk_toggle(&path, &hunk.id.clone());
    }

    pub(in crate::tui::sigil_view) fn hunk_toggle(&self, path: &str, hunk: &str) {
        let accepted = self.hunk_note(path, hunk).and_then(|note| note.verdict)
            == Some(crate::hunks::Verdict::Accepted);
        self.hunk_mark(
            path,
            hunk,
            crate::hunks::Op::Verdict((!accepted).then_some(crate::hunks::Verdict::Accepted)),
        );
    }

    pub(in crate::tui::sigil_view) fn hunk_reject(&self) {
        let Some(path) = self.diff_view.peek() else {
            return;
        };
        let hunks = self.hunks();
        let Some(hunk) = hunks.get(self.hunk_cursor.peek().min(hunks.len().saturating_sub(1)))
        else {
            return;
        };
        self.hunk_mark(&path, &hunk.id.clone(), crate::hunks::Op::Reject);
    }

    pub(in crate::tui::sigil_view) fn hunk_mark(
        &self,
        path: &str,
        hunk: &str,
        op: crate::hunks::Op,
    ) {
        drop(self.cmd.send(Cmd::Review {
            root: self.root(),
            path: path.to_string(),
            hunk: hunk.to_string(),
            op,
        }));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn hunk_context(&self, index: usize) {
        let Some(path) = self.diff_view.peek() else {
            return;
        };
        let hunks = self.hunks();
        let Some(hunk) = hunks.get(index) else {
            return;
        };
        self.hunk_cursor.set(index);
        self.pane_focus.set(true);
        self.context.set(Some(Context {
            col: 0,
            row: 0,
            cursor: 0,
            target: FileRow {
                path,
                name: hunk.header.clone(),
                depth: 0,
                dir: false,
                open: false,
            },
            kind: Pane::Diff,
            hunk: Some(hunk.id.clone()),
        }));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn note_start(&self, path: String, hunk: String) {
        let existing = self
            .hunk_note(&path, &hunk)
            .and_then(|note| note.comment)
            .unwrap_or_default();
        self.note_text.set(existing.clone());
        self.note_caret.set(existing.len());
        self.note_target.set(Some((path, hunk)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn note_commit(&self, text: &str) {
        let Some((path, hunk)) = self.note_target.peek() else {
            return;
        };
        self.note_target.set(None);
        self.note_text.set(String::new());
        self.hunk_mark(
            &path,
            &hunk,
            crate::hunks::Op::Comment(Some(text.to_string())),
        );
    }

    pub(in crate::tui::sigil_view) fn note_stop(&self) -> bool {
        if self.note_target.peek().is_none() {
            return false;
        }
        self.note_target.set(None);
        self.note_text.set(String::new());
        self.refresh();
        true
    }

    pub(in crate::tui::sigil_view) fn diff_open(&self, path: String) {
        self.model_mut(|model| model.patch = None);
        self.hunk_cursor.set(0);
        self.note_target.set(None);
        self.diff_view.set(Some(path.clone()));
        drop(self.cmd.send(Cmd::Patch(self.root(), path)));
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn root(&self) -> Option<PathBuf> {
        self.diff_root.peek().map(|(path, _)| path)
    }

    pub(in crate::tui::sigil_view) fn compare(&self, root: Option<(PathBuf, String)>) {
        self.model_mut(|model| {
            model.patch = None;
            model.marks = None;
        });
        self.hunk_cursor.set(0);
        self.note_target.set(None);
        self.diff_root.set(root);
    }

    pub(in crate::tui::sigil_view) fn tree_rows(&self) -> Vec<FileRow> {
        self.model
            .get()
            .trees
            .iter()
            .map(|row| FileRow {
                path: row.tree.path.display().to_string(),
                name: row.tree.branch.clone(),
                depth: 0,
                dir: false,
                open: false,
            })
            .collect()
    }

    pub(in crate::tui::sigil_view) fn tree_move(&self, delta: isize) {
        let count = self.tree_rows().len();
        if count == 0 {
            return;
        }
        let cursor = self.tree_cursor.peek().min(count - 1) as isize;
        self.tree_cursor
            .set((cursor + delta).clamp(0, count as isize - 1) as usize);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn tree_enter(&self) {
        let rows = self.tree_rows();
        let cursor = self.tree_cursor.peek().min(rows.len().saturating_sub(1));
        if let Some(row) = rows.get(cursor) {
            self.tree_show(PathBuf::from(&row.path), row.name.clone());
        }
    }

    pub(in crate::tui::sigil_view) fn tree_show(&self, path: PathBuf, branch: String) {
        self.compare(Some((path, branch)));
        self.diff_view.set(None);
        let layout = self.layout.peek();
        self.set_layout(Layout {
            secondary: Some(Pane::Diff),
            ..layout
        });
        self.pane_focus.set(true);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn diff_command(&self, argument: Option<&str>) {
        self.splash.set(false);
        self.compare(None);
        let layout = self.layout.peek();
        if layout.secondary != Some(Pane::Diff) {
            self.set_layout(Layout {
                secondary: Some(Pane::Diff),
                ..layout
            });
        }
        self.pane_focus.set(true);
        match argument {
            Some(path) => self.diff_open(path.to_string()),
            None => {
                self.diff_view.set(None);
                self.refresh();
            }
        }
    }
}
