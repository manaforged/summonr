use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::diff::{git_env, git_output};
use crate::durable::store;
use crate::error::{Error, Result};

#[derive(Clone)]
pub(crate) struct Checkpoints {
    root: PathBuf,
    index: PathBuf,
    log: PathBuf,
    skip: PathBuf,
}

impl Checkpoints {
    pub(crate) fn open(workspace: &Path, session: &str) -> Option<Self> {
        let dir = git_env(workspace, &[], &["rev-parse", "--absolute-git-dir"]).ok()?;
        let dir = PathBuf::from(dir.trim()).join("summonr");
        std::fs::create_dir_all(&dir).ok()?;
        let index = dir.join(format!("{session}.index"));
        if !index.exists() {
            seed(workspace, &index);
        }
        Some(Self {
            root: workspace.to_path_buf(),
            index,
            log: dir.join(format!("{session}.checkpoints")),
            skip: dir.join("skip"),
        })
    }

    pub(crate) fn capture(&self) -> Result<()> {
        let tree = self.tree()?;
        let mut trees = self.trees();
        trees.push(tree);
        store::write(&self.log, format!("{}\n", trees.join("\n")))?;
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
        let changes = git_output(
            &self.root,
            &[],
            &[
                "diff-tree",
                "-r",
                "--no-renames",
                "--name-status",
                "-z",
                target,
                &current,
            ],
            None,
        )?;
        let mut paths = Vec::new();
        let mut records = changes.split(|byte| *byte == 0);
        while let Some(status) = records.next().filter(|status| !status.is_empty()) {
            let path = records.next().ok_or_else(|| {
                Error::Config(format!(
                    "git diff-tree returned an incomplete path for {}",
                    String::from_utf8_lossy(status)
                ))
            })?;
            let path = String::from_utf8_lossy(path).into_owned();
            if !crate::diff::ignored::hidden(&path) {
                paths.push(path);
            }
        }
        let env = self.env();
        git_env(&self.root, &env, &["read-tree", "--reset", "-u", target])?;
        Ok(paths)
    }

    fn skipped(&self) -> Result<Vec<String>> {
        let mut paths: Vec<String> = std::fs::read_to_string(&self.skip)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect();
        let known = paths.len();
        for path in crate::durable::tree::own_files(&self.root) {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        if paths.len() > known {
            store::write(&self.skip, format!("{}\n", paths.join("\n")))?;
        }
        Ok(paths)
    }

    pub(crate) fn tree(&self) -> Result<String> {
        let env = self.env();
        let skipped = self.skipped()?;
        let excluded: Vec<String> = skipped
            .iter()
            .map(|path| format!(":(exclude,literal){path}"))
            .collect();
        let mut add = vec!["add", "-A", "--", "."];
        add.extend(excluded.iter().map(String::as_str));
        git_env(&self.root, &env, &add)?;
        if !skipped.is_empty() {
            let literal: Vec<String> = skipped
                .iter()
                .map(|path| format!(":(literal){path}"))
                .collect();
            let mut drop = vec!["rm", "--cached", "-f", "-q", "--ignore-unmatch", "--"];
            drop.extend(literal.iter().map(String::as_str));
            git_env(&self.root, &env, &drop)?;
        }
        Ok(git_env(&self.root, &env, &["write-tree"])?
            .trim()
            .to_string())
    }

    pub(crate) fn trees(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn env(&self) -> [(&str, &OsStr); 1] {
        [("GIT_INDEX_FILE", self.index.as_os_str())]
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
