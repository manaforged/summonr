use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::diff::{git_env, git_output};
use crate::durable::store;
use summonr_core::error::{Error, Result};

const CHECKPOINTS: &str = "checkpoints";
const STORE: &str = "git";
const EXCLUDE: &str = "info/exclude";
const READY: &str = "summonr-ready";
const KEY_BYTES: usize = 8;

#[derive(Clone)]
pub(crate) struct Checkpoints {
    root: PathBuf,
    index: PathBuf,
    log: PathBuf,
    skip: PathBuf,
    store: Option<PathBuf>,
}

impl Checkpoints {
    pub(crate) fn open(workspace: &Path, session: &str) -> Option<Self> {
        let (dir, store) = match crate::diff::state_dir(workspace) {
            Some(dir) => (dir, None),
            None => {
                let dir = private_dir(workspace)?;
                (dir.clone(), Some(dir.join(STORE)))
            }
        };
        std::fs::create_dir_all(&dir).ok()?;
        let index = dir.join(format!("{session}.index"));
        match &store {
            Some(_) => {}
            None if !index.exists() => match crate::diff::layout(workspace) {
                Some(layout) => seed_from(workspace, &layout, &index),
                None => seed(workspace, &index),
            },
            None => {}
        }
        Some(Self {
            root: workspace.to_path_buf(),
            index,
            log: dir.join(format!("{session}.checkpoints")),
            skip: dir.join("skip"),
            store,
        })
    }

    pub(crate) fn capture(&self) -> Result<()> {
        self.capture_to(self.count()? + 1)
    }

    pub(crate) fn capture_to(&self, turn: usize) -> Result<()> {
        if self.trees()?.len() >= turn {
            return Ok(());
        }
        let tree = self.tree()?;
        let _lock = store::lock(&self.log, true)?;
        let mut trees = self.trees()?;
        if trees.len() >= turn {
            return Ok(());
        }
        trees.resize(turn, tree);
        write_lines(&self.log, &trees)
    }

    /// Adds the current content of `paths` to turn `turn`'s checkpoint before a tool rewrites
    /// them. A folder outside Git has no full capture, so only files the agent's tools edit and
    /// files an earlier capture holds are restorable.
    pub(crate) fn capture_paths(&self, turn: usize, paths: &[String]) -> Result<()> {
        if self.store.is_none() || turn == 0 {
            return Ok(());
        }
        self.capture_to(turn)?;
        let wanted: Vec<String> = paths
            .iter()
            .filter(|path| Path::new(path).is_relative() && self.root.join(path).is_file())
            .map(|path| format!(":(literal){path}"))
            .collect();
        if wanted.is_empty() {
            return Ok(());
        }
        let mut known = vec!["ls-files", "-z", "--"];
        known.extend(wanted.iter().map(String::as_str));
        let tracked: std::collections::HashSet<String> = self.paths(&known)?.into_iter().collect();
        let fresh: Vec<&String> = wanted
            .iter()
            .filter(|spec| !tracked.contains(spec.trim_start_matches(":(literal)")))
            .collect();
        if fresh.is_empty() {
            return Ok(());
        }
        let _lock = store::lock(&self.log, true)?;
        let mut add = vec!["add", "-f", "--"];
        add.extend(fresh.iter().map(|spec| spec.as_str()));
        self.git(&add)?;
        let tree = self.git(&["write-tree"])?.trim().to_string();
        let mut trees = self.trees()?;
        if let Some(slot) = trees.get_mut(turn - 1) {
            *slot = tree;
        }
        write_lines(&self.log, &trees)
    }

    pub(crate) fn deferred(&self) -> bool {
        self.store.is_some()
    }

    pub(crate) fn count(&self) -> Result<usize> {
        Ok(self.trees()?.len())
    }

    pub(crate) fn restore(&self, turn: usize) -> Result<Vec<String>> {
        let trees = self.trees()?;
        let target = turn
            .checked_sub(1)
            .and_then(|slot| trees.get(slot))
            .ok_or_else(|| {
                Error::Config(format!(
                    "no checkpoint for turn {turn}; this session has {}",
                    trees.len()
                ))
            })?;
        let current = self.tree()?;
        let mut paths = self.paths(&[
            "diff-tree",
            "-r",
            "--no-renames",
            "--name-only",
            "-z",
            target,
            &current,
        ])?;
        paths.retain(|path| !crate::diff::ignored::hidden(path));
        self.git(&["read-tree", "--reset", "-u", target])?;
        Ok(paths)
    }

    fn skipped(&self) -> Result<Vec<String>> {
        let mut paths = read_lines(&self.skip)?;
        let known = paths.len();
        let mut seen: std::collections::HashSet<String> = paths.iter().cloned().collect();
        for path in crate::durable::tree::own_files(&self.root)? {
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
        if paths.len() > known {
            write_lines(&self.skip, &paths)?;
        }
        Ok(paths)
    }

    pub(crate) fn tree(&self) -> Result<String> {
        if let Some(store) = &self.store {
            init(&self.root, store)?;
        }
        let skipped = self.skipped()?;
        let excluded: Vec<String> = skipped
            .iter()
            .map(|path| format!(":(exclude,literal){path}"))
            .collect();
        let add = if self.store.is_some() {
            vec!["add", "-u"]
        } else {
            let mut all = vec!["add", "-A", "--", "."];
            all.extend(excluded.iter().map(String::as_str));
            all
        };
        self.git(&add)?;
        if !skipped.is_empty() {
            let literal: Vec<String> = skipped
                .iter()
                .map(|path| format!(":(literal){path}"))
                .collect();
            let mut drop = vec!["rm", "--cached", "-f", "-q", "--ignore-unmatch", "--"];
            drop.extend(literal.iter().map(String::as_str));
            self.git(&drop)?;
        }
        Ok(self.git(&["write-tree"])?.trim().to_string())
    }

    pub(crate) fn trees(&self) -> Result<Vec<String>> {
        read_lines(&self.log)
    }

    pub(crate) fn git(&self, args: &[&str]) -> Result<String> {
        git_env(&self.root, &self.env(), args)
    }

    pub(crate) fn paths(&self, args: &[&str]) -> Result<Vec<String>> {
        Ok(crate::diff::nul_paths(&git_output(
            &self.root,
            &self.env(),
            args,
            None,
        )?))
    }

    fn env(&self) -> Vec<(&str, &OsStr)> {
        let mut env = vec![("GIT_INDEX_FILE", self.index.as_os_str())];
        if let Some(store) = &self.store {
            env.push(("GIT_DIR", store.as_os_str()));
            env.push(("GIT_WORK_TREE", self.root.as_os_str()));
        }
        env
    }
}

fn private_dir(workspace: &Path) -> Option<PathBuf> {
    let workspace = dunce::canonicalize(workspace).ok()?;
    let home = summonr_core::dirs::summonr_home().ok()?;
    if home.starts_with(&workspace) {
        return None;
    }
    let key = Sha256::digest(workspace.as_os_str().as_encoded_bytes());
    let key: String = key
        .iter()
        .take(KEY_BYTES)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(home.join(CHECKPOINTS).join(key))
}

fn init(workspace: &Path, git_dir: &Path) -> Result<()> {
    let ready = git_dir.join(READY);
    if ready.exists() {
        return Ok(());
    }
    let _lock = store::lock(git_dir, true)?;
    if ready.exists() {
        return Ok(());
    }
    let path = OsString::from(git_dir);
    let path = path.to_str().ok_or_else(|| {
        Error::Config(format!(
            "cannot create the checkpoint store at {}: the path is not UTF-8",
            git_dir.display()
        ))
    })?;
    git_env(workspace, &[], &["init", "-q", "--bare", path])?;
    let exclude: String = summonr_core::walk::SKIP_DIRS
        .iter()
        .map(|dir| format!("{dir}/\n"))
        .collect();
    store::write(&git_dir.join(EXCLUDE), exclude, store::Durability::Renamed)?;
    store::write(&ready, "", store::Durability::Renamed)?;
    Ok(())
}

fn seed_from(workspace: &Path, layout: &crate::diff::Layout, index: &Path) {
    let at_top = dunce::canonicalize(workspace).is_ok_and(|path| path == layout.top);
    if at_top && std::fs::copy(layout.index(), index).is_err() {
        drop(std::fs::remove_file(index));
    }
}

fn seed(workspace: &Path, index: &Path) {
    let Ok(text) = git_env(
        workspace,
        &[],
        &["rev-parse", "--git-path", "index", "--show-prefix"],
    ) else {
        return;
    };
    let mut lines = text.lines();
    let Some(source) = lines.next() else {
        return;
    };
    if lines.next().is_some_and(|prefix| !prefix.trim().is_empty()) {
        return;
    }
    if std::fs::copy(workspace.join(source.trim()), index).is_err() {
        drop(std::fs::remove_file(index));
    }
}

fn read_lines(path: &Path) -> Result<Vec<String>> {
    let text = store::read_or_empty(path)
        .map_err(|error| Error::Config(format!("cannot read {}: {error}", path.display())))?;
    Ok(text.lines().map(str::to_string).collect())
}

fn write_lines(path: &Path, lines: &[String]) -> Result<()> {
    store::write(
        path,
        format!("{}\n", lines.join("\n")),
        store::Durability::Renamed,
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
