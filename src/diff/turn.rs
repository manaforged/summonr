use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;

use super::{Diff, Stat, git, git_paths, ignored, numstat_row};
use crate::checkpoint::Checkpoints;
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnId {
    Last,
    Number(usize),
    Session,
}

const SESSION_TOKEN: &str = "session";
pub const SESSION_BASE: &str = "all turns";

impl FromStr for TurnId {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self> {
        if text == "last" {
            return Ok(Self::Last);
        }
        if text == SESSION_TOKEN {
            return Ok(Self::Session);
        }
        match text.parse::<usize>() {
            Ok(number) if number > 0 => Ok(Self::Number(number)),
            Ok(_) | Err(_) => Err(Error::Config(format!(
                "--turn takes a turn number, `last`, or `session`, not `{text}`"
            ))),
        }
    }
}

struct Span {
    base: String,
    from: String,
    to: String,
}

impl Span {
    fn stats(&self, root: &Path, session: &str) -> Result<Diff> {
        let numstat = git(
            root,
            &[
                "diff",
                "--numstat",
                "--no-renames",
                &self.from,
                &self.to,
                "--",
            ],
        )?;
        let added: HashSet<String> = git_paths(
            root,
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                "--diff-filter=A",
                &self.from,
                &self.to,
                "--",
            ],
        )?
        .into_iter()
        .collect();
        let mut files: Vec<Stat> = numstat
            .lines()
            .filter_map(numstat_row)
            .filter(|stat| !ignored::hidden(&stat.path))
            .map(|stat| Stat {
                new: added.contains(&stat.path),
                ..stat
            })
            .collect();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Diff {
            base: self.base.clone(),
            session: Some(session.to_string()),
            files,
        })
    }
}

fn spans(root: &Path, session: &str) -> Result<Vec<Span>> {
    let points = Checkpoints::open(root, session)
        .ok_or_else(|| Error::Config("--turn needs a git workspace".to_string()))?;
    let trees = points.trees();
    if trees.is_empty() {
        return Err(Error::Config(
            "the session has no recorded turns in this workspace".to_string(),
        ));
    }
    let current = points.tree()?;
    Ok(trees
        .iter()
        .enumerate()
        .map(|(slot, from)| Span {
            base: format!("turn {}", slot + 1),
            from: from.clone(),
            to: trees.get(slot + 1).unwrap_or(&current).clone(),
        })
        .collect())
}

fn pick(root: &Path, session: &str, id: TurnId) -> Result<(Span, Diff)> {
    let mut spans = spans(root, session)?;
    let count = spans.len();
    match id {
        TurnId::Number(number) if number <= count => {
            let span = spans.swap_remove(number - 1);
            let diff = span.stats(root, session)?;
            Ok((span, diff))
        }
        TurnId::Number(number) => Err(Error::Config(format!(
            "no turn {number}; the session has {}",
            crate::text::plural(count, "turn")
        ))),
        TurnId::Session => {
            let to = spans.last().map(|span| span.to.clone()).unwrap_or_default();
            let mut span = spans.swap_remove(0);
            span.to = to;
            span.base = SESSION_BASE.to_string();
            let diff = span.stats(root, session)?;
            Ok((span, diff))
        }
        TurnId::Last => {
            let mut empty = None;
            while let Some(span) = spans.pop() {
                let diff = span.stats(root, session)?;
                if !diff.files.is_empty() {
                    return Ok((span, diff));
                }
                empty.get_or_insert((span, diff));
            }
            empty.ok_or_else(|| Error::Config("the session has no turns".to_string()))
        }
    }
}

pub fn turn_stats(root: &Path, session: &str, id: TurnId) -> Result<Diff> {
    pick(root, session, id).map(|(_, diff)| diff)
}

pub fn turn_patch(root: &Path, session: &str, id: TurnId, path: Option<&str>) -> Result<String> {
    let (span, diff) = pick(root, session, id)?;
    let shown: Vec<String> = diff
        .files
        .iter()
        .filter(|stat| path.is_none_or(|path| path == stat.path))
        .map(|stat| format!(":(literal){}", stat.path))
        .collect();
    if shown.is_empty() {
        return Ok(String::new());
    }
    let mut args = vec![
        "diff",
        "--no-color",
        "--no-renames",
        span.from.as_str(),
        span.to.as_str(),
        "--",
    ];
    args.extend(shown.iter().map(String::as_str));
    let mut text = git(root, &args)?;
    text.push('\n');
    Ok(text)
}
