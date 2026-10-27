use std::path::Path;

use ignore::{DirEntry, WalkBuilder};

use crate::{SKIP_DIRS, STATE_DIRS};

const SKIP_ROOTS: &[&str] = &["/System/Volumes/Data", "/proc", "/sys", "/dev"];

#[derive(Clone, Copy)]
pub(crate) struct Walk {
    pub(crate) gitignore: bool,
    pub(crate) dotfiles: bool,
    pub(crate) skip: &'static [&'static str],
    pub(crate) guard: bool,
    pub(crate) depth: Option<usize>,
    pub(crate) sorted: bool,
}

impl Walk {
    pub(crate) const TOOL: Self = Self {
        gitignore: true,
        dotfiles: true,
        skip: SKIP_DIRS,
        guard: false,
        depth: None,
        sorted: false,
    };

    pub(crate) const RAW: Self = Self {
        gitignore: false,
        skip: STATE_DIRS,
        ..Self::TOOL
    };

    pub(crate) fn builder(self, root: &Path) -> WalkBuilder {
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

    pub(crate) fn entries(self, root: &Path) -> impl Iterator<Item = DirEntry> + use<> {
        self.builder(root).build().filter_map(Result::ok)
    }

    fn prunes(self, dir: &Path) -> bool {
        dir.file_name()
            .is_some_and(|name| self.skip.contains(&name.to_string_lossy().as_ref()))
            || (self.guard
                && (SKIP_ROOTS.iter().any(|root| dir == Path::new(root))
                    || crate::policy::protected_read(dir)))
    }
}

pub(crate) fn is_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_some_and(|kind| kind.is_dir())
}

pub(crate) fn listed(entry: &DirEntry) -> bool {
    entry
        .file_type()
        .is_some_and(|kind| kind.is_file() || (kind.is_symlink() && !entry.path().is_dir()))
}

pub(crate) fn children(dir: &Path) -> impl Iterator<Item = std::fs::DirEntry> + use<> {
    std::fs::read_dir(dir).into_iter().flatten().flatten()
}
