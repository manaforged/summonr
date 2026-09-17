use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::diff::git_env;
use crate::durable::store;
use crate::error::{Error, Result};

pub(crate) struct Checkpoints {
    root: PathBuf,
    index: PathBuf,
    log: PathBuf,
}

impl Checkpoints {
    pub(crate) fn open(workspace: &Path, session: &str) -> Option<Self> {
        let dir = git_env(workspace, &[], &["rev-parse", "--absolute-git-dir"]).ok()?;
        let dir = PathBuf::from(dir.trim()).join("summonr");
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self {
            root: workspace.to_path_buf(),
            index: dir.join(format!("{session}.index")),
            log: dir.join(format!("{session}.checkpoints")),
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
        let top = git_env(&self.root, &[], &["rev-parse", "--show-toplevel"])?;
        let top = Path::new(top.trim());
        let changes = git_env(
            &self.root,
            &[],
            &[
                "diff-tree",
                "-r",
                "--no-renames",
                "--name-status",
                target,
                &current,
            ],
        )?;
        let mut paths = Vec::new();
        for line in changes.lines() {
            let Some((status, path)) = line.split_once('\t') else {
                continue;
            };
            if status == "A" {
                std::fs::remove_file(top.join(path))?;
            }
            paths.push(path.to_string());
        }
        let env = self.env();
        git_env(top, &env, &["read-tree", target])?;
        git_env(top, &env, &["checkout-index", "-a", "-f"])?;
        Ok(paths)
    }

    fn tree(&self) -> Result<String> {
        let env = self.env();
        git_env(&self.root, &env, &["add", "-A", "--", "."])?;
        Ok(git_env(&self.root, &env, &["write-tree"])?
            .trim()
            .to_string())
    }

    fn trees(&self) -> Vec<String> {
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
