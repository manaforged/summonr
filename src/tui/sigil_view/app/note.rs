use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::tui::sigil_view) struct Note {
    pub(in crate::tui::sigil_view) text: String,
    pub(in crate::tui::sigil_view) failed: bool,
}

impl Note {
    pub(in crate::tui::sigil_view) const fn plain(text: String) -> Self {
        Self {
            text,
            failed: false,
        }
    }

    pub(super) const fn failed(text: String) -> Self {
        Self { text, failed: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) struct Reverted {
    pub(in crate::tui::sigil_view) path: String,
    pub(in crate::tui::sigil_view) hunk: String,
    pub(in crate::tui::sigil_view) header: String,
}

impl App {
    pub(in crate::tui::sigil_view) fn focus_composer(&self) {
        self.editor_focus
            .set(self.editor_focus.peek().wrapping_add(1));
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

    pub(in crate::tui::sigil_view) fn current_hunk(&self) -> Option<(String, String)> {
        let path = self.diff_view.peek()?;
        let hunks = self.hunks();
        let hunk = hunks.get(self.hunk_cursor.peek().min(hunks.len().saturating_sub(1)))?;
        Some((path, hunk.id.clone()))
    }

    pub(in crate::tui::sigil_view) fn hunk_accept(&self) {
        if let Some((path, hunk)) = self.current_hunk() {
            self.hunk_toggle(&path, &hunk);
        }
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
        if let Some((path, hunk)) = self.current_hunk() {
            self.hunk_mark(&path, &hunk, crate::hunks::Op::Reject);
        }
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

    pub(in crate::tui::sigil_view) fn hunk_revert(&self, path: &str, hunk: &str) {
        self.hunk_mark(path, hunk, crate::hunks::Op::Reject);
    }

    pub(in crate::tui::sigil_view) fn hunk_restore(&self, path: &str, hunk: &str) {
        self.hunk_mark(path, hunk, crate::hunks::Op::Restore);
    }

    pub(in crate::tui::sigil_view) fn file_revert(&self, path: &str) {
        self.hunk_mark(path, "", crate::hunks::Op::RejectFile);
    }

    pub(in crate::tui::sigil_view) fn file_review(&self, path: &str) -> (usize, usize) {
        let model = self.model.peek();
        let Some(hunks) = model
            .marks
            .as_ref()
            .filter(|(root, _)| *root == self.root())
            .and_then(|(_, marks)| marks.get(path))
        else {
            return (0, 0);
        };
        let count = |verdict| {
            hunks
                .values()
                .filter(|note| note.verdict == Some(verdict))
                .count()
        };
        (
            count(crate::hunks::Verdict::Accepted),
            count(crate::hunks::Verdict::Rejected),
        )
    }

    pub(in crate::tui::sigil_view) fn reverted(&self, only: Option<&str>) -> Vec<Reverted> {
        let model = self.model.peek();
        let Some((_, marks)) = model
            .marks
            .as_ref()
            .filter(|(root, _)| *root == self.root())
        else {
            return Vec::new();
        };
        marks
            .iter()
            .filter(|(path, _)| only.is_none_or(|only| only == path.as_str()))
            .flat_map(|(path, hunks)| {
                hunks.iter().filter_map(move |(id, note)| {
                    let patch = note.patch.as_ref()?;
                    (note.verdict == Some(crate::hunks::Verdict::Rejected)).then(|| Reverted {
                        path: path.clone(),
                        hunk: id.clone(),
                        header: patch
                            .lines()
                            .find(|line| line.starts_with("@@ "))
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
            })
            .collect()
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
        self.pane_focus.set(true);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn note_commit(&self, text: &str) {
        let Some((path, hunk)) = self.note_target.peek() else {
            return;
        };
        self.note_target.set(None);
        self.note_text.set(String::new());
        self.focus_composer();
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
        self.focus_composer();
        self.refresh();
        true
    }
}
