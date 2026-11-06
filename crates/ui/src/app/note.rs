use sigil::widgets::CursorEdge;

use super::*;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Note {
    pub(crate) text: String,
    pub(crate) failed: bool,
}

impl Note {
    pub(crate) const fn plain(text: String) -> Self {
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
pub(crate) struct Reverted {
    pub(crate) path: String,
    pub(crate) hunk: String,
    pub(crate) header: String,
}

impl App {
    pub(crate) fn focus_composer(&self) {
        self.editor_focus
            .set(self.editor_focus.peek().wrapping_add(1));
    }

    pub(crate) fn hunk_note(&self, path: &str, hunk: &str) -> Option<summonr_agent::hunks::Note> {
        let model = self.model.peek();
        let (root, marks) = model.marks.as_ref()?;
        if *root != self.root() {
            return None;
        }
        marks.get(path)?.get(hunk).cloned()
    }

    pub(crate) fn hunk_move(&self, delta: isize) {
        self.hunk_list
            .shift(delta, self.hunks().len(), CursorEdge::Clamp);
        self.refresh();
    }

    pub(crate) fn current_hunk(&self) -> Option<(String, String)> {
        let path = self.diff_view.peek()?;
        let hunks = self.hunks();
        let hunk = hunks.get(self.hunk_list.at(hunks.len()))?;
        Some((path, hunk.id.clone()))
    }

    pub(crate) fn hunk_accept(&self) {
        if let Some((path, hunk)) = self.current_hunk() {
            self.hunk_toggle(&path, &hunk);
        }
    }

    pub(crate) fn hunk_toggle(&self, path: &str, hunk: &str) {
        let accepted = self.hunk_note(path, hunk).and_then(|note| note.verdict)
            == Some(summonr_agent::hunks::Verdict::Accepted);
        self.hunk_mark(
            path,
            hunk,
            summonr_agent::hunks::Op::Verdict(
                (!accepted).then_some(summonr_agent::hunks::Verdict::Accepted),
            ),
        );
    }

    pub(crate) fn hunk_reject(&self) {
        if let Some((path, hunk)) = self.current_hunk() {
            self.hunk_mark(&path, &hunk, summonr_agent::hunks::Op::Reject);
        }
    }

    pub(crate) fn hunk_mark(&self, path: &str, hunk: &str, op: summonr_agent::hunks::Op) {
        drop(self.cmd.send(Cmd::Review {
            root: self.root(),
            path: path.to_string(),
            hunk: hunk.to_string(),
            op,
        }));
        self.refresh();
    }

    pub(crate) fn hunk_revert(&self, path: &str, hunk: &str) {
        self.hunk_mark(path, hunk, summonr_agent::hunks::Op::Reject);
    }

    pub(crate) fn hunk_restore(&self, path: &str, hunk: &str) {
        self.hunk_mark(path, hunk, summonr_agent::hunks::Op::Restore);
    }

    pub(crate) fn file_revert(&self, path: &str) {
        if !confirm(&self.revert_armed, path.to_string()) {
            self.refresh();
            return;
        }
        self.hunk_mark(path, "", summonr_agent::hunks::Op::RejectFile);
    }

    pub(crate) fn file_review(&self, path: &str) -> (usize, usize) {
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
            count(summonr_agent::hunks::Verdict::Accepted),
            count(summonr_agent::hunks::Verdict::Rejected),
        )
    }

    pub(crate) fn reverted(&self, only: Option<&str>) -> Vec<Reverted> {
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
                    (note.verdict == Some(summonr_agent::hunks::Verdict::Rejected)).then(|| {
                        Reverted {
                            path: path.clone(),
                            hunk: id.clone(),
                            header: patch
                                .lines()
                                .find(|line| line.starts_with("@@ "))
                                .unwrap_or_default()
                                .to_string(),
                        }
                    })
                })
            })
            .collect()
    }

    pub(crate) fn note_start(&self, path: String, hunk: String) {
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

    pub(crate) fn note_commit(&self, text: &str) {
        let Some((path, hunk)) = self.note_target.peek() else {
            return;
        };
        self.note_target.set(None);
        self.note_text.set(String::new());
        self.focus_composer();
        self.hunk_mark(
            &path,
            &hunk,
            summonr_agent::hunks::Op::Comment(Some(text.to_string())),
        );
    }

    pub(crate) fn note_stop(&self) -> bool {
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
