use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

const FINGERPRINT_MAX: u64 = 256 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Entry(u64, u64, u32);

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Snapshot(BTreeMap<String, Entry>);

pub(crate) fn snapshot(root: &Path, exclude: Option<&Path>) -> io::Result<Snapshot> {
    let mut files = BTreeMap::new();
    for relative in listing(root)? {
        let path = root.join(&relative);
        if exclude.is_some_and(|exclude| path.starts_with(exclude)) {
            continue;
        }
        let Ok(metadata) = path.symlink_metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .unwrap_or_default();
        let mark = if metadata.len() <= FINGERPRINT_MAX {
            std::fs::read(&path).map_or(modified.subsec_nanos(), |bytes| {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                bytes.hash(&mut hasher);
                hasher.finish() as u32
            })
        } else {
            modified.subsec_nanos()
        };
        files.insert(
            relative.to_string_lossy().replace('\\', "/"),
            Entry(metadata.len(), modified.as_secs(), mark),
        );
    }
    Ok(Snapshot(files))
}

impl Snapshot {
    #[cfg(test)]
    pub(crate) fn with(paths: &[&str]) -> Self {
        Self(
            paths
                .iter()
                .enumerate()
                .map(|(index, path)| ((*path).to_string(), Entry(index as u64 + 1, 0, 0)))
                .collect(),
        )
    }

    pub(crate) fn matches(&self, other: &Snapshot, paths: &[String]) -> bool {
        paths
            .iter()
            .all(|path| self.0.get(path) == other.0.get(path))
    }
}

pub(crate) fn changed(base: &Snapshot, capture: &Snapshot) -> Vec<String> {
    let mut paths: Vec<String> = capture
        .0
        .iter()
        .filter(|(path, entry)| base.0.get(*path) != Some(*entry))
        .map(|(path, _)| path.clone())
        .collect();
    paths.extend(
        base.0
            .keys()
            .filter(|path| !capture.0.contains_key(*path))
            .cloned(),
    );
    paths.sort();
    paths
}

pub(crate) fn listing(root: &Path) -> io::Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        return Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(|name| PathBuf::from(String::from_utf8_lossy(name).into_owned()))
            .collect());
    }
    let mut found = Vec::new();
    walk(root, root, &mut found)?;
    Ok(found)
}

pub(crate) fn walked(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    walk(root, root, &mut found)?;
    Ok(found)
}

fn walk(root: &Path, dir: &Path, found: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            let name = entry.file_name();
            if crate::SKIP_DIRS.iter().any(|skip| name == *skip) {
                continue;
            }
            walk(root, &path, found)?;
        } else if kind.is_file()
            && let Ok(relative) = path.strip_prefix(root)
        {
            found.push(relative.to_path_buf());
        }
    }
    Ok(())
}
