use std::path::Path;

use ignore::{DirEntry, WalkBuilder};

pub const STATE_DIRS: &[&str] = &[".git", ".summonr"];
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    ".summonr",
    "target",
    "node_modules",
    "dist",
    "__pycache__",
];

const SKIP_ROOTS: &[&str] = &["/System/Volumes/Data", "/proc", "/sys", "/dev"];

#[derive(Clone, Copy)]
pub struct Walk {
    pub gitignore: bool,
    pub dotfiles: bool,
    pub skip: &'static [&'static str],
    pub guard: Option<fn(&Path) -> bool>,
    pub depth: Option<usize>,
    pub sorted: bool,
}

impl Walk {
    pub const TOOL: Self = Self {
        gitignore: true,
        dotfiles: true,
        skip: SKIP_DIRS,
        guard: None,
        depth: None,
        sorted: false,
    };

    pub const RAW: Self = Self {
        gitignore: false,
        skip: STATE_DIRS,
        ..Self::TOOL
    };

    pub fn builder(self, root: &Path) -> WalkBuilder {
        let mut walker = WalkBuilder::new(root);
        walker
            .standard_filters(self.gitignore)
            .hidden(!self.dotfiles)
            .require_git(false)
            .max_depth(self.depth)
            .filter_entry(move |entry| {
                entry.depth() == 0 || !is_dir(entry) || !self.prunes(entry.path())
            });
        if self.sorted {
            walker.sort_by_file_name(std::ffi::OsStr::cmp);
        }
        walker
    }

    pub fn entries(self, root: &Path) -> impl Iterator<Item = DirEntry> + use<> {
        self.builder(root).build().filter_map(Result::ok)
    }

    fn prunes(self, dir: &Path) -> bool {
        dir.file_name()
            .is_some_and(|name| self.skip.contains(&name.to_string_lossy().as_ref()))
            || self.guard.is_some_and(|protected| {
                SKIP_ROOTS.iter().any(|root| dir == Path::new(root)) || protected(dir)
            })
    }
}

pub fn is_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_some_and(|kind| kind.is_dir())
}

pub fn listed(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|kind| kind.is_file() || (kind.is_symlink() && !entry.path().is_dir()))
}

pub fn children(dir: &Path) -> impl Iterator<Item = std::fs::DirEntry> + use<> {
    std::fs::read_dir(dir).into_iter().flatten().flatten()
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
