use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff;
use crate::durable::store;
use summonr_core::error::{Error, Result};
use summonr_core::text::Fnv64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub(crate) id: String,
    pub(crate) header: String,
    pub(crate) patch: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Verdict {
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Note {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
}

pub(crate) type Marks = BTreeMap<String, BTreeMap<String, Note>>;

pub(crate) enum Op {
    Verdict(Option<Verdict>),
    Comment(Option<String>),
    Reject,
    RejectFile,
    Restore,
}

const FILE: &str = "review.json";

fn file(root: &Path) -> PathBuf {
    crate::diff::state_dir(root)
        .map_or_else(|| root.join(".summonr").join(FILE), |dir| dir.join(FILE))
}

fn doc(root: &Path) -> store::Doc<store::PrettyJson<Marks>> {
    store::Doc::new(
        file(root),
        store::Durability::Renamed,
        store::Invalid::Warn,
        store::Wait::Forever,
    )
}

pub(crate) fn load(root: &Path) -> Marks {
    doc(root).load().unwrap_or_default()
}

impl Hunk {
    pub(crate) fn body(&self) -> &str {
        self.patch
            .match_indices("@@ ")
            .find(|(at, _)| *at == 0 || self.patch.as_bytes()[at - 1] == b'\n')
            .map_or(self.patch.as_str(), |(at, _)| &self.patch[at..])
    }
}

pub(crate) fn split(path: &str, patch: &str) -> Vec<Hunk> {
    let mut head = String::new();
    let mut bodies: Vec<String> = Vec::new();
    for line in patch.lines() {
        if line.starts_with("@@ ") {
            bodies.push(String::new());
        }
        let target = bodies.last_mut().unwrap_or(&mut head);
        target.push_str(line);
        target.push('\n');
    }
    bodies
        .into_iter()
        .map(|body| {
            let header = body.lines().next().unwrap_or_default().to_string();
            let mut hash = Fnv64::default();
            hash.write(path.as_bytes());
            for line in body.lines().skip(1) {
                hash.write_u8(b'\n');
                hash.write(line.as_bytes());
            }
            Hunk {
                id: format!("{:016x}", hash.finish()),
                header,
                patch: format!("{head}{body}"),
            }
        })
        .collect()
}

fn note<'a>(marks: &'a mut Marks, path: &str, hunk: &str) -> &'a mut Note {
    marks
        .entry(path.to_string())
        .or_default()
        .entry(hunk.to_string())
        .or_default()
}

fn prune(marks: &mut Marks) {
    marks.retain(|_, hunks| {
        hunks.retain(|_, note| note.verdict.is_some() || note.comment.is_some());
        !hunks.is_empty()
    });
}

pub(crate) fn apply(root: &Path, path: &str, hunk: &str, op: Op) -> Result<Marks> {
    let mut saved = Marks::new();
    doc(root).update(|marks| {
        apply_op(root, marks, path, hunk, op)?;
        prune(marks);
        saved = marks.clone();
        Ok(true)
    })?;
    Ok(saved)
}

fn apply_op(root: &Path, marks: &mut Marks, path: &str, hunk: &str, op: Op) -> Result<()> {
    match op {
        Op::Verdict(verdict) => note(marks, path, hunk).verdict = verdict,
        Op::Comment(comment) => {
            note(marks, path, hunk).comment = comment.filter(|text| !text.trim().is_empty());
        }
        Op::Reject => {
            let patch = diff::patch(root, Some(path))?;
            let Some(target) = split(path, &patch).into_iter().find(|item| item.id == hunk) else {
                return Err(Error::Config(format!(
                    "the hunk is gone; {path} changed since it was loaded"
                )));
            };
            diff::git_apply(root, target.patch.as_bytes(), &["--reverse"])?;
            rejected(marks, path, target);
        }
        Op::RejectFile => {
            let patch = diff::patch(root, Some(path))?;
            if patch.trim().is_empty() {
                return Err(Error::Config(format!("{path} has no changes to revert")));
            }
            diff::git_apply(root, patch.as_bytes(), &["--reverse"])?;
            for target in split(path, &patch) {
                rejected(marks, path, target);
            }
        }
        Op::Restore => {
            let entry = note(marks, path, hunk);
            let Some(patch) = entry.patch.clone() else {
                return Err(Error::Config(format!(
                    "nothing to undo for this hunk in {path}"
                )));
            };
            diff::git_apply(root, patch.as_bytes(), &[])?;
            *entry = Note {
                comment: entry.comment.take(),
                ..Note::default()
            };
        }
    }
    Ok(())
}

fn rejected(marks: &mut Marks, path: &str, target: Hunk) {
    let entry = note(marks, path, &target.id);
    entry.verdict = Some(Verdict::Rejected);
    entry.reported = false;
    entry.patch = Some(target.patch);
}

pub(crate) fn drain(root: &Path) -> Option<String> {
    let mut lines = Vec::new();
    let saved = doc(root).update(|marks| {
        for (path, hunks) in marks.iter_mut() {
            for (id, note) in hunks.iter_mut() {
                let hunk = id.get(..8).unwrap_or(id);
                if note.verdict == Some(Verdict::Rejected) && !note.reported {
                    note.reported = true;
                    lines.push(format!(
                        "{path} hunk {hunk}: the user reverted this hunk; do not reapply it unless asked"
                    ));
                }
                if let Some(comment) = note.comment.take() {
                    lines.push(format!("{path} hunk {hunk}: {comment}"));
                }
            }
        }
        if lines.is_empty() {
            return Ok(false);
        }
        prune(marks);
        Ok(true)
    });
    drop(saved);
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "[review notes on the diff pane: address each one or say why not]\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
#[path = "hunks_tests.rs"]
mod tests;
