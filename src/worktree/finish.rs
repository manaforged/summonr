use crate::error::{Error, Result};

use super::{COMMIT_MAX, Tree, dirty};

const SUBJECT_MAX: usize = 60;

pub fn finish(tree: &Tree, objective: &str, paths: &[String]) -> Result<Vec<String>> {
    let mut committed = Vec::new();
    if dirty(&tree.path)? {
        let tracked = crate::diff::git_paths(&tree.path, &["ls-files", "-z"])?;
        let tracked: std::collections::HashSet<&str> = tracked.iter().map(String::as_str).collect();
        let small: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|path| !crate::diff::ignored::hidden(path))
            .filter(|path| {
                std::fs::metadata(tree.path.join(path)).map_or_else(
                    |_missing| tracked.contains(path),
                    |meta| meta.len() <= COMMIT_MAX,
                )
            })
            .collect();
        if !small.is_empty() {
            let mut args = vec!["--literal-pathspecs", "add", "--all", "--"];
            args.extend(small.iter().copied());
            crate::diff::git_env(&tree.path, &[], &args)?;
            crate::diff::git_env(
                &tree.path,
                &[],
                &["commit", "-q", "-m", &subject(objective)],
            )?;
            committed = small.iter().map(|path| (*path).to_string()).collect();
        }
        let leftover = leftover(&tree.path)?;
        if !leftover.is_empty() {
            return Err(Error::Config(format!(
                "left uncommitted in {} (not part of this run, or over {} bytes):\n{}",
                tree.path.display(),
                COMMIT_MAX,
                leftover.join("\n")
            )));
        }
    }
    let big: Vec<String> = crate::diff::git_paths(
        &tree.primary,
        &[
            "diff",
            "--name-only",
            "-z",
            &format!("{}..{}", tree.base, tree.branch),
        ],
    )?
    .into_iter()
    .filter(|path| {
        std::fs::metadata(tree.path.join(path)).is_ok_and(|meta| meta.len() > COMMIT_MAX)
    })
    .collect();
    if !big.is_empty() {
        return Err(Error::Config(format!(
            "files over {COMMIT_MAX} bytes on {}: {}; remove them from the branch before it can merge",
            tree.branch,
            big.join(", ")
        )));
    }
    let head = crate::diff::git_env(&tree.primary, &[], &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if head != tree.base {
        return Err(Error::Config(format!(
            "{} is on {head}; the run started on {}",
            tree.primary.display(),
            tree.base
        )));
    }
    crate::diff::git_env(
        &tree.primary,
        &[],
        &["merge", "--ff-only", "--", &tree.branch],
    )?;
    tree.remove()?;
    Ok(committed)
}

fn leftover(root: &std::path::Path) -> Result<Vec<String>> {
    let hidden = crate::diff::ignored::hidden;
    let mut fields = crate::diff::git_paths(
        root,
        &["status", "--porcelain", "-z", "--untracked-files=all"],
    )?
    .into_iter();
    let mut left = Vec::new();
    while let Some(record) = fields.next() {
        let moved = matches!(record.as_bytes().first(), Some(b'R' | b'C'));
        let from = moved.then(|| fields.next()).flatten();
        let Some(path) = record.get(3..) else {
            continue;
        };
        let source = from.filter(|_| record.starts_with('R'));
        if !hidden(path) || source.is_some_and(|from| !hidden(&from)) {
            left.push(record);
        }
    }
    Ok(left)
}

fn subject(objective: &str) -> String {
    let mut text = String::new();
    for word in objective.split_whitespace() {
        let spaced = usize::from(!text.is_empty());
        if text.len() + spaced + word.len() > SUBJECT_MAX {
            break;
        }
        if spaced == 1 {
            text.push(' ');
        }
        text.push_str(word);
    }
    if text.is_empty() {
        text = objective.trim().chars().take(SUBJECT_MAX).collect();
    }
    let mut chars = text.trim_end_matches(['.', ' ']).chars();
    let head: String = chars
        .next()
        .map(|c| c.to_lowercase().collect())
        .unwrap_or_default();
    format!("summonr: {head}{}", chars.as_str())
}
