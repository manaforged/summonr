use std::path::Path;

use crate::error::{Error, Result};
use crate::tools::{Hold, Kept};

use super::{COMMIT_MAX, Tree, base_record, worktree_id};

pub(crate) struct Landing {
    pub(crate) paths: Vec<String>,
    pub(crate) kept: Option<String>,
    pub(crate) held: Option<Kept>,
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
    if !tree.on_base() {
        return tree.kept(
            Hold::OffBase,
            "moved off the parent base; its delta was kept",
        );
    }
    let (bytes, found) = match tree.patch(paths) {
        Ok(patch) => patch,
        Err(error) => return tree.kept(Hold::Unreadable, &format!("was kept: {error}")),
    };
    if bytes.is_empty() && !found.is_empty() {
        return tree.kept(
            Hold::Unreadable,
            "changed files that produced no patch; it was kept",
        );
    }
    if !bytes.is_empty()
        && let Err(error) = apply(target, &bytes)
    {
        return tree.kept(
            Hold::Conflict,
            &format!("conflicts with the parent and was kept: {error}"),
        );
    }
    tree.landed(if bytes.is_empty() { Vec::new() } else { found })
}

impl Tree {
    fn kept(&self, hold: Hold, reason: &str) -> Landing {
        Landing {
            paths: Vec::new(),
            kept: Some(format!("{} {reason}", self.path.display())),
            held: Some(self.held(hold)),
        }
    }

    fn held(&self, hold: Hold) -> Kept {
        Kept {
            checkout: self.path.clone(),
            hold,
        }
    }

    fn landed(&self, paths: Vec<String>) -> Landing {
        let kept = self.remove_child().err().map(|error| {
            format!(
                "applied changes, but could not remove {}: {error}",
                self.path.display()
            )
        });
        Landing {
            paths,
            held: kept.as_ref().map(|_| self.held(Hold::Unremoved)),
            kept,
        }
    }

    fn on_base(&self) -> bool {
        let Ok(head) = crate::diff::git_env(&self.path, &[], &["rev-parse", "HEAD"]) else {
            return false;
        };
        let Ok(base) = crate::diff::git_env(&self.path, &[], &["merge-base", &self.base, &head])
        else {
            return false;
        };
        base == self.base
    }

    fn place(&self) -> &'static str {
        if self.snapshot.is_some() {
            "delta"
        } else {
            "worktree"
        }
    }

    fn patch(&self, paths: &[String]) -> Result<(Vec<u8>, Vec<String>)> {
        let discovered = if self.snapshot.is_some() {
            Some(self.snapshot_paths()?)
        } else {
            None
        };
        let paths = discovered.as_deref().unwrap_or(paths);
        if paths.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        for path in paths {
            if std::fs::metadata(self.path.join(path)).is_ok_and(|meta| meta.len() > COMMIT_MAX) {
                return Err(Error::Config(format!(
                    "{path} exceeds {COMMIT_MAX} bytes; the child {} was kept",
                    self.place()
                )));
            }
        }
        let mut list = vec![
            "--literal-pathspecs",
            "ls-files",
            "-z",
            "--others",
            "--exclude-standard",
            "--",
        ];
        list.extend(paths.iter().map(String::as_str));
        let untracked = crate::diff::git_paths(&self.path, &list)?;
        if !untracked.is_empty() {
            let mut mark = vec!["--literal-pathspecs", "add", "-N", "--"];
            mark.extend(untracked.iter().map(String::as_str));
            crate::diff::git_env(&self.path, &[], &mark)?;
        }
        let base = self.snapshot.as_deref().unwrap_or("HEAD");
        let mut diff = vec![
            "--literal-pathspecs",
            "diff",
            "--binary",
            "--no-ext-diff",
            base,
            "--",
        ];
        diff.extend(paths.iter().map(String::as_str));
        let bytes = crate::diff::git_output(&self.path, &[], &diff, None)?;
        Ok((bytes, paths.to_vec()))
    }

    fn snapshot_paths(&self) -> Result<Vec<String>> {
        let snapshot = self.snapshot.as_deref().unwrap_or("HEAD");
        let mut paths =
            crate::diff::git_paths(&self.path, &["diff", "--name-only", "-z", snapshot])?;
        paths.extend(crate::diff::git_paths(
            &self.path,
            &["ls-files", "-z", "--others", "--exclude-standard"],
        )?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub(crate) fn remove_child(&self) -> Result<()> {
        if self.snapshot.is_some() {
            if self.path.exists() {
                std::fs::remove_dir_all(&self.path)?;
            }
            if let Some(parent) = self.path.parent() {
                drop(std::fs::remove_dir(parent));
            }
            return Ok(());
        }
        let head = crate::diff::git_env(&self.primary, &[], &["rev-parse", &self.branch])?;
        let path = self.path.to_str().ok_or_else(|| {
            Error::Config(format!(
                "worktree path is not utf-8: {}",
                self.path.display()
            ))
        })?;
        crate::diff::git_env(&self.primary, &[], &["worktree", "remove", "--force", path])?;
        crate::diff::git_env(
            &self.primary,
            &[],
            &[
                "update-ref",
                "-d",
                &format!("refs/heads/{}", self.branch),
                &head,
            ],
        )?;
        if let Some(id) = worktree_id(&self.branch)
            && let Ok(record) = base_record(&self.primary, id)
        {
            drop(std::fs::remove_file(record));
        }
        if let Some(parent) = self.path.parent() {
            drop(std::fs::remove_dir(parent));
        }
        Ok(())
    }
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
                tree.place()
            )));
        }
    }
    Ok(start)
}

fn combine_children(children: &[(Tree, Vec<String>)]) -> Result<Vec<u8>> {
    let patches = children
        .iter()
        .map(|(tree, paths)| tree.patch(paths).map(|(bytes, _)| bytes))
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
    children[0].0.patch(&paths).map(|(bytes, _)| bytes)
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
        .map(|(tree, paths)| tree.landed(paths.clone()))
        .collect()
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
