use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::diff::{git_env, git_output};
use crate::durable::store;
use summonr_core::error::{Error, Result};

const CHECKPOINTS: &str = "checkpoints";
const STORE: &str = "git";
const EXCLUDE: &str = "info/exclude";
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
            Some(store) => init(workspace, store)?,
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
        let tree = self.tree()?;
        let mut trees = self.trees();
        trees.push(tree);
        store::write(
            &self.log,
            format!("{}\n", trees.join("\n")),
            store::Durability::Renamed,
        )?;
        Ok(())
    }

    pub(crate) fn count(&self) -> usize {
        self.trees().len()
    }

    pub(crate) fn restore(&self, turn: usize) -> Result<Vec<String>> {
        let trees = self.trees();
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
        let mut paths: Vec<String> = std::fs::read_to_string(&self.skip)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        let known = paths.len();
        let mut seen: std::collections::HashSet<String> = paths.iter().cloned().collect();
        for path in crate::durable::tree::own_files(&self.root) {
            if seen.insert(path.clone()) {
                paths.push(path);
            }
        }
        if paths.len() > known {
            store::write(
                &self.skip,
                format!("{}\n", paths.join("\n")),
                store::Durability::Renamed,
            )?;
        }
        Ok(paths)
    }

    pub(crate) fn tree(&self) -> Result<String> {
        let skipped = self.skipped()?;
        let excluded: Vec<String> = skipped
            .iter()
            .map(|path| format!(":(exclude,literal){path}"))
            .collect();
        let mut add = vec!["add", "-A", "--", "."];
        add.extend(excluded.iter().map(String::as_str));
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

    pub(crate) fn trees(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
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

fn init(workspace: &Path, store: &Path) -> Option<()> {
    if store.exists() {
        return Some(());
    }
    let path = OsString::from(store);
    git_env(workspace, &[], &["init", "-q", "--bare", path.to_str()?]).ok()?;
    let exclude: String = summonr_core::walk::SKIP_DIRS
        .iter()
        .map(|dir| format!("{dir}/\n"))
        .collect();
    std::fs::write(store.join(EXCLUDE), exclude).ok()
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

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
