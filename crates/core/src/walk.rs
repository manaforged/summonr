use std::path::Path;

use ignore::{DirEntry, WalkBuilder};

/// Folders a raw walk still skips.
pub const STATE_DIRS: &[&str] = &[".git", crate::dirs::STATE];
/// Folders a tool walk skips.
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    crate::dirs::STATE,
    "target",
    "node_modules",
    "dist",
    "__pycache__",
];

const SKIP_ROOTS: &[&str] = &["/System/Volumes/Data", "/proc", "/sys", "/dev"];

/// The settings of a directory walk.
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
    /// The walk the tools use: gitignore on, dotfiles in, build folders skipped.
    pub const TOOL: Self = Self {
        gitignore: true,
        dotfiles: true,
        skip: SKIP_DIRS,
        guard: None,
        depth: None,
        sorted: false,
    };

    /// The tool walk without gitignore, skipping only state folders.
    pub const RAW: Self = Self {
        gitignore: false,
        skip: STATE_DIRS,
        ..Self::TOOL
    };

    /// The raw walk when `no_ignore` is set, else the tool walk.
    #[must_use]
    pub const fn tool(no_ignore: bool) -> Self {
        if no_ignore { Self::RAW } else { Self::TOOL }
    }

    /// An `ignore` walk builder for `root` with these settings.
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

    /// The readable entries under `root`.
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

/// Whether `entry` is a directory.
pub fn is_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_some_and(|kind| kind.is_dir())
}

/// Whether `entry` is a file, or a symlink that is not to a directory.
pub fn listed(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|kind| kind.is_file() || (kind.is_symlink() && !entry.path().is_dir()))
}

/// The readable entries directly inside `dir`.
pub fn children(dir: &Path) -> impl Iterator<Item = std::fs::DirEntry> + use<> {
    std::fs::read_dir(dir).into_iter().flatten().flatten()
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
