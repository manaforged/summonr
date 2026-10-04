use std::collections::BTreeMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::diff;
use crate::durable::store;
use crate::error::{Error, Result};
use crate::text::Fnv64;

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
}

pub(crate) type Marks = BTreeMap<String, BTreeMap<String, Note>>;

pub(crate) enum Op {
    Verdict(Option<Verdict>),
    Comment(Option<String>),
    Reject,
}

fn file(root: &Path) -> PathBuf {
    root.join(".summonr").join("review.json")
}

pub(crate) fn load(root: &Path) -> Marks {
    std::fs::read_to_string(file(root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save(root: &Path, marks: &Marks) -> Result<()> {
    let path = file(root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    store::write(&path, serde_json::to_string_pretty(marks)?)?;
    Ok(())
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
    let mut marks = load(root);
    match op {
        Op::Verdict(verdict) => note(&mut marks, path, hunk).verdict = verdict,
        Op::Comment(comment) => {
            note(&mut marks, path, hunk).comment = comment.filter(|text| !text.trim().is_empty());
        }
        Op::Reject => {
            let patch = diff::patch(root, Some(path))?;
            let Some(target) = split(path, &patch).into_iter().find(|item| item.id == hunk) else {
                return Err(Error::Config(format!(
                    "the hunk is gone — {path} changed since it was loaded"
                )));
            };
            diff::git_output(
                root,
                &[],
                &["apply", "--reverse", "--whitespace=nowarn"],
                Some(target.patch.as_bytes()),
            )?;
            note(&mut marks, path, hunk).verdict = Some(Verdict::Rejected);
        }
    }
    prune(&mut marks);
    save(root, &marks)?;
    Ok(marks)
}

pub(crate) fn drain(root: &Path) -> Option<String> {
    let mut marks = load(root);
    let mut lines = Vec::new();
    for (path, hunks) in marks.iter_mut() {
        for (id, note) in hunks.iter_mut() {
            if let Some(comment) = note.comment.take() {
                lines.push(format!(
                    "{path} hunk {}: {comment}",
                    id.get(..8).unwrap_or(id)
                ));
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    prune(&mut marks);
    drop(save(root, &marks));
    Some(format!(
        "[review notes left on the diff pane — address each or say why not]\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
#[path = "hunks_tests.rs"]
mod tests;
