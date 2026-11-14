use std::path::{Path, PathBuf};

use summonr_core::error::Result;

use crate::diff::git_paths;

const INCLUDE: &str = ".worktreeinclude";
const DEFAULT: &str = ".env*";

pub(super) fn patterns(root: &Path, globs: Option<&str>) -> Result<Vec<String>> {
    let own = match std::fs::read_to_string(root.join(INCLUDE)) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    let text = own.as_deref().or(globs).unwrap_or(DEFAULT);
    Ok(text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}

pub(super) fn copy(patterns: &[String], from: &Path, to: &Path) -> Result<Vec<PathBuf>> {
    if patterns.is_empty() || same(from, to) {
        return Ok(Vec::new());
    }
    let ignored = list(from, &["--exclude-standard", "--directory"])?;
    let excludes: Vec<String> = patterns
        .iter()
        .map(|line| format!("--exclude={line}"))
        .collect();
    let matched = list(
        from,
        &excludes.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let mut copied = Vec::new();
    for file in matched.iter().filter(|file| under_any(file, &ignored)) {
        let target = to.join(file);
        if target.symlink_metadata().is_ok() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from.join(file), &target)?;
        copied.push(PathBuf::from(file));
    }
    Ok(copied)
}

fn list(root: &Path, rules: &[&str]) -> Result<Vec<String>> {
    let mut args = vec!["ls-files", "-z", "--others", "--ignored"];
    args.extend_from_slice(rules);
    git_paths(root, &args)
}

fn under_any(file: &str, ignored: &[String]) -> bool {
    !file.ends_with('/')
        && ignored.iter().any(|entry| {
            entry == file || (entry.ends_with('/') && file.starts_with(entry.as_str()))
        })
}

fn same(one: &Path, other: &Path) -> bool {
    let real = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    real(one) == real(other)
}
