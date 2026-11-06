use summonr_core::error::{Error, Result};

use super::{COMMIT_MAX, Tree, dirty};

const SUBJECT_MAX: usize = 60;

pub fn finish(tree: &Tree, own: &str, objective: &str, paths: &[String]) -> Result<Vec<String>> {
    if let Some(session) = tree.holder(own)? {
        return Err(Error::Config(format!("session {session} still uses it")));
    }
    let base = tree.known_base()?;
    commit(tree, objective, paths)?;
    let changed = crate::diff::git_paths(
        &tree.primary,
        &[
            "diff",
            "--name-only",
            "-z",
            &format!("{base}...{}", tree.branch),
        ],
    )?;
    let big: Vec<&str> = changed
        .iter()
        .map(String::as_str)
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
    checked_head(tree, &base)?;
    crate::diff::git_env(
        &tree.primary,
        &[],
        &["merge", "--ff-only", "--", &tree.branch],
    )?;
    tree.remove()?;
    Ok(changed)
}

/// Commits everything in the worktree, merges its branch into the checkout, and removes it.
pub(crate) fn merge_all(tree: &Tree, own: &str, objective: &str) -> Result<()> {
    if let Some(session) = tree.holder(own)? {
        return Err(Error::Config(format!(
            "agent {} still uses this worktree; stop it first",
            crate::session::short(&session)
        )));
    }
    let base = tree.known_base()?;
    let paths: Vec<String> = leftover(&tree.path)?
        .iter()
        .filter_map(|record| record.get(3..).map(str::to_owned))
        .collect();
    commit(tree, objective, &paths)?;
    let head = checked_head(tree, &base)?;
    let git = |args: &[&str]| crate::diff::git_env(&tree.primary, &[], args);
    if git(&["merge", "--ff-only", "--", &tree.branch]).is_err()
        && let Err(error) = git(&["merge", "--no-edit", "--", &tree.branch])
    {
        drop(git(&["merge", "--abort"]));
        return Err(Error::Config(format!(
            "{} does not merge cleanly into {head}, so it was kept: {error}",
            tree.branch
        )));
    }
    tree.remove()
}

fn checked_head(tree: &Tree, base: &str) -> Result<String> {
    let head = crate::diff::git_env(&tree.primary, &[], &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if !started_on(tree, &head, base) {
        return Err(Error::Config(format!(
            "{} is on {head}; the run started on {base}",
            tree.primary.display(),
        )));
    }
    Ok(head)
}

fn commit(tree: &Tree, objective: &str, paths: &[String]) -> Result<()> {
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
            let staged =
                crate::diff::git_paths(&tree.path, &["diff", "--cached", "--name-only", "-z"])?;
            if !staged.is_empty() {
                crate::diff::git_env(
                    &tree.path,
                    &[],
                    &["commit", "-q", "-m", &subject(objective)],
                )?;
            }
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
    Ok(())
}

fn started_on(tree: &Tree, head: &str, base: &str) -> bool {
    let git = |args: &[&str]| crate::diff::git_env(&tree.primary, &[], args).is_ok();
    if git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("refs/heads/{base}"),
    ]) {
        return head == base;
    }
    head != "HEAD" && git(&["merge-base", "--is-ancestor", base, "HEAD"])
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
