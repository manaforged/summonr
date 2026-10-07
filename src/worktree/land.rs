use std::path::Path;

use crate::error::{Error, Result};

use super::{COMMIT_MAX, Tree, base_record, dirty, remove, worktree_id};

pub(crate) struct Landing {
    pub(crate) paths: Vec<String>,
    pub(crate) kept: Option<String>,
}

pub(crate) fn reconcile_children(
    target: &Path,
    children: &[(Tree, Vec<String>)],
) -> Result<Vec<Landing>> {
    if children.is_empty() {
        return Ok(Vec::new());
    }
    if children.iter().all(|(tree, _)| tree.snapshot.is_some()) {
        return Ok(children
            .iter()
            .map(|(tree, paths)| reconcile_delta(target, tree, paths))
            .collect());
    }
    let start = children_base(target, children)?;
    let combined = combine_children(children)?;
    let deltas = children.iter().all(|(tree, _)| tree.snapshot.is_some());
    apply_combined(target, &start, &combined, deltas)?;
    Ok(remove_landings(children))
}

fn reconcile_delta(target: &Path, tree: &Tree, paths: &[String]) -> Landing {
    if !on_base(tree) {
        return kept(tree, "moved off the parent base; its delta was kept");
    }
    let (bytes, found) = match patch(tree, paths) {
        Ok(patch) => patch,
        Err(error) => return kept(tree, &format!("was kept: {error}")),
    };
    if !bytes.is_empty()
        && let Err(error) = apply(target, &bytes)
    {
        return kept(
            tree,
            &format!("conflicts with the parent and was kept: {error}"),
        );
    }
    let note = remove_child(tree).err().map(|error| {
        format!(
            "applied changes, but could not remove {}: {error}",
            tree.path.display()
        )
    });
    Landing {
        paths: if bytes.is_empty() { Vec::new() } else { found },
        kept: note,
    }
}

fn kept(tree: &Tree, reason: &str) -> Landing {
    Landing {
        paths: Vec::new(),
        kept: Some(format!("{} {reason}", tree.path.display())),
    }
}

fn on_base(tree: &Tree) -> bool {
    let Ok(head) = crate::diff::git_env(&tree.path, &[], &["rev-parse", "HEAD"]) else {
        return false;
    };
    let Ok(base) = crate::diff::git_env(&tree.path, &[], &["merge-base", &tree.base, &head]) else {
        return false;
    };
    base == tree.base
}

fn children_base(target: &Path, children: &[(Tree, Vec<String>)]) -> Result<String> {
    let start = crate::diff::git_env(target, &[], &["rev-parse", "HEAD"])?;
    if !children.iter().all(|(tree, _)| tree.snapshot.is_some()) {
        let dirty = crate::diff::git_env(
            target,
            &[],
            &["status", "--porcelain", "--untracked-files=no"],
        )?;
        if !dirty.is_empty() {
            return Err(Error::Config(
                "the parent tracked files changed while writable children ran; their worktrees were kept"
                    .to_string(),
            ));
        }
    }
    for (tree, _) in children {
        let head = crate::diff::git_env(&tree.path, &[], &["rev-parse", "HEAD"])?;
        if head != start {
            return Err(Error::Config(format!(
                "{} moved from the parent base; its {} was kept",
                tree.branch,
                place(tree)
            )));
        }
    }
    Ok(start)
}

fn place(tree: &Tree) -> &'static str {
    if tree.snapshot.is_some() {
        "delta"
    } else {
        "worktree"
    }
}

fn combine_children(children: &[(Tree, Vec<String>)]) -> Result<Vec<u8>> {
    let patches = children
        .iter()
        .map(|(tree, paths)| patch(tree, paths).map(|(bytes, _)| bytes))
        .collect::<Result<Vec<_>>>()?;
    let integration = &children[0].0.path;
    for bytes in patches.iter().skip(1).filter(|bytes| !bytes.is_empty()) {
        apply(integration, bytes).map_err(|error| {
            Error::Config(format!(
                "writable child changes conflict in {}: {error}",
                integration.display()
            ))
        })?;
    }
    let mut paths = children
        .iter()
        .flat_map(|(_, paths)| paths.iter().cloned())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    patch(&children[0].0, &paths).map(|(bytes, _)| bytes)
}

fn apply_combined(target: &Path, start: &str, combined: &[u8], deltas: bool) -> Result<()> {
    let current = crate::diff::git_env(target, &[], &["rev-parse", "HEAD"])?;
    let dirty = crate::diff::git_env(
        target,
        &[],
        &["status", "--porcelain", "--untracked-files=no"],
    )?;
    if current != start || (!deltas && !dirty.is_empty()) {
        return Err(Error::Config(format!(
            "the parent tracked files changed before reconciliation; child {} were kept",
            if deltas { "deltas" } else { "worktrees" }
        )));
    }
    if !combined.is_empty() {
        apply(target, combined).map_err(|error| {
            Error::Config(format!(
                "cannot apply writable child changes to {}: {error}",
                target.display()
            ))
        })?;
    }
    Ok(())
}

fn remove_landings(children: &[(Tree, Vec<String>)]) -> Vec<Landing> {
    children
        .iter()
        .map(|(tree, paths)| Landing {
            paths: paths.clone(),
            kept: remove_child(tree).err().map(|error| {
                format!(
                    "applied changes, but could not remove {}: {error}",
                    tree.path.display()
                )
            }),
        })
        .collect()
}

fn patch(tree: &Tree, paths: &[String]) -> Result<(Vec<u8>, Vec<String>)> {
    let discovered = if tree.snapshot.is_some() {
        Some(snapshot_paths(tree)?)
    } else {
        None
    };
    let paths = discovered.as_deref().unwrap_or(paths);
    if paths.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    for path in paths {
        if std::fs::metadata(tree.path.join(path)).is_ok_and(|meta| meta.len() > COMMIT_MAX) {
            return Err(Error::Config(format!(
                "{path} exceeds {COMMIT_MAX} bytes; the child {} was kept",
                place(tree)
            )));
        }
    }
    let mut list = vec![
        "--literal-pathspecs",
        "ls-files",
        "--others",
        "--exclude-standard",
        "--",
    ];
    list.extend(paths.iter().map(String::as_str));
    let untracked = crate::diff::git_env(&tree.path, &[], &list)?;
    if !untracked.is_empty() {
        let mut mark = vec!["--literal-pathspecs", "add", "-N", "--"];
        mark.extend(untracked.lines());
        crate::diff::git_env(&tree.path, &[], &mark)?;
    }
    let base = tree.snapshot.as_deref().unwrap_or("HEAD");
    let mut diff = vec![
        "--literal-pathspecs",
        "diff",
        "--binary",
        "--no-ext-diff",
        base,
        "--",
    ];
    diff.extend(paths.iter().map(String::as_str));
    let bytes = crate::diff::git_output(&tree.path, &[], &diff, None)?;
    Ok((bytes, paths.to_vec()))
}

fn snapshot_paths(tree: &Tree) -> Result<Vec<String>> {
    let snapshot = tree.snapshot.as_deref().unwrap_or("HEAD");
    let diff = crate::diff::git_env(&tree.path, &[], &["diff", "--name-only", snapshot])?;
    let untracked = crate::diff::git_env(
        &tree.path,
        &[],
        &["ls-files", "--others", "--exclude-standard"],
    )?;
    let mut paths: Vec<String> = diff
        .lines()
        .chain(untracked.lines())
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn apply(tree: &Path, patch: &[u8]) -> Result<()> {
    crate::diff::git_output(
        tree,
        &[],
        &["apply", "--check", "--binary", "--whitespace=nowarn", "-"],
        Some(patch),
    )?;
    crate::diff::git_output(
        tree,
        &[],
        &["apply", "--binary", "--whitespace=nowarn", "-"],
        Some(patch),
    )?;
    Ok(())
}

pub(crate) fn remove_child(tree: &Tree) -> Result<()> {
    if tree.snapshot.is_some() {
        if tree.path.exists() {
            std::fs::remove_dir_all(&tree.path)?;
        }
        if let Some(parent) = tree.path.parent() {
            drop(std::fs::remove_dir(parent));
        }
        return Ok(());
    }
    let head = crate::diff::git_env(&tree.primary, &[], &["rev-parse", &tree.branch])?;
    let path = tree.path.to_str().ok_or_else(|| {
        Error::Config(format!(
            "worktree path is not utf-8: {}",
            tree.path.display()
        ))
    })?;
    crate::diff::git_env(&tree.primary, &[], &["worktree", "remove", "--force", path])?;
    crate::diff::git_env(
        &tree.primary,
        &[],
        &[
            "update-ref",
            "-d",
            &format!("refs/heads/{}", tree.branch),
            &head,
        ],
    )?;
    if let Some(id) = worktree_id(&tree.branch)
        && let Ok(record) = base_record(&tree.primary, id)
    {
        drop(std::fs::remove_file(record));
    }
    if let Some(parent) = tree.path.parent() {
        drop(std::fs::remove_dir(parent));
    }
    Ok(())
}

pub fn finish(tree: &Tree, objective: &str, paths: &[String]) -> Result<Vec<String>> {
    let mut committed = Vec::new();
    if dirty(&tree.path)? {
        let tracked = crate::diff::git_env(&tree.path, &[], &["ls-files"])?;
        let tracked: std::collections::HashSet<&str> = tracked.lines().collect();
        let small: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|path| {
                std::fs::metadata(tree.path.join(path)).map_or_else(
                    |_missing| tracked.contains(path),
                    |meta| meta.len() <= COMMIT_MAX,
                )
            })
            .collect();
        if !small.is_empty() {
            let mut args = vec!["add", "--all", "--"];
            args.extend(small.iter().copied());
            crate::diff::git_env(&tree.path, &[], &args)?;
            crate::diff::git_env(
                &tree.path,
                &[],
                &["commit", "-q", "-m", &subject(objective)],
            )?;
            committed = small.iter().map(|path| (*path).to_string()).collect();
        }
        let leftover = crate::diff::git_env(&tree.path, &[], &["status", "--porcelain"])?;
        if !leftover.is_empty() {
            return Err(Error::Config(format!(
                "left uncommitted in {} (not part of this run, or over {} bytes):\n{leftover}",
                tree.path.display(),
                COMMIT_MAX
            )));
        }
    }
    let big: Vec<String> = crate::diff::git_env(
        &tree.primary,
        &[],
        &[
            "diff",
            "--name-only",
            &format!("{}..{}", tree.base, tree.branch),
        ],
    )?
    .lines()
    .filter(|path| {
        std::fs::metadata(tree.path.join(path)).is_ok_and(|meta| meta.len() > COMMIT_MAX)
    })
    .map(str::to_string)
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
    remove(tree)?;
    Ok(committed)
}

fn subject(objective: &str) -> String {
    let text: String = objective
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(41)
        .collect();
    let mut chars = text.trim_end_matches(['.', ' ']).chars();
    let head: String = chars
        .next()
        .map(|c| c.to_lowercase().collect())
        .unwrap_or_default();
    format!("summonr: {head}{}", chars.as_str())
}
