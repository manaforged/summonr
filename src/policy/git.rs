#[cfg(any(target_os = "linux", windows, test))]
use std::path::{Path, PathBuf};

pub(crate) const GIT_DIR: &str = ".git";
pub(crate) const GIT_MODULES: &str = "modules";
pub(crate) const GIT_GUARDS: &[&str] = &[
    "hooks",
    "config",
    "commondir",
    "HEAD",
    "index",
    "refs",
    "packed-refs",
    "logs",
];
#[cfg(any(target_os = "linux", windows, test))]
const GIT_DATA: &[&str] = &["objects", "refs", "logs", "lfs"];

pub(super) fn guarded_git_part(parts: &[&str]) -> bool {
    parts.iter().enumerate().any(|(at, part)| {
        *part == GIT_DIR
            && match parts.get(at + 1..).unwrap_or_default() {
                [first, ..] if GIT_GUARDS.contains(first) => true,
                [first, rest @ ..] if *first == GIT_MODULES => {
                    rest.iter().skip(1).any(|part| GIT_GUARDS.contains(part))
                }
                _ => false,
            }
    })
}

#[cfg(any(target_os = "linux", windows, test))]
#[must_use]
pub(crate) fn guarded_git_paths(workspace: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect(&workspace.join(GIT_DIR), &mut found);
    found
}

#[cfg(any(target_os = "linux", windows, test))]
fn collect(dir: &Path, found: &mut Vec<PathBuf>) {
    found.extend(
        GIT_GUARDS
            .iter()
            .map(|guard| dir.join(guard))
            .filter(|path| path.exists()),
    );
    let Ok(entries) = std::fs::read_dir(dir.join(GIT_MODULES)) else {
        return;
    };
    for entry in entries.flatten() {
        collect_module(&entry.path(), found);
    }
}

#[cfg(any(target_os = "linux", windows, test))]
fn collect_module(dir: &Path, found: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    if dir.join("HEAD").is_file() {
        collect(dir, found);
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !GIT_DATA.iter().any(|data| name == *data) {
            collect_module(&entry.path(), found);
        }
    }
}

#[cfg(any(target_os = "macos", test))]
#[must_use]
pub(crate) fn guarded_git_regex(workspace: &str) -> String {
    let escaped: String = workspace
        .chars()
        .flat_map(|c| {
            let meta = ".^$*+?()[]{}|\\".contains(c);
            meta.then_some('\\').into_iter().chain([c])
        })
        .collect();
    let names: Vec<&str> = GIT_GUARDS.to_vec();
    format!(
        "^{escaped}/\\{GIT_DIR}/({GIT_MODULES}/.+/)?({})(/.*)?$",
        names.join("|")
    )
}
