use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use notify::event::{EventKind, ModifyKind};
use notify::{Event as NotifyEvent, RecursiveMode, Watcher};

use crate::diff;
use crate::error::{Error, Result};

pub(super) fn walk(root: &Path, depth: Option<usize>) -> impl Iterator<Item = ignore::DirEntry> {
    WalkBuilder::new(root)
        .require_git(false)
        .max_depth(depth)
        .build()
        .filter_map(std::result::Result::ok)
}

pub(super) fn watch(watcher: &mut impl Watcher, dirs: Vec<PathBuf>) -> notify::Result<()> {
    if dirs.is_empty() {
        return Ok(());
    }
    let mut paths = watcher.paths_mut();
    for entry in dirs.iter().flat_map(|dir| walk(dir, None)) {
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            paths.add(entry.path(), RecursiveMode::NonRecursive)?;
        }
    }
    paths.commit()
}

pub(super) fn files(workspace: &Path) -> Vec<String> {
    let Ok(root) = std::fs::canonicalize(workspace) else {
        return Vec::new();
    };
    walk(&root, None)
        .filter_map(|entry| {
            let file_type = entry.file_type()?;
            (file_type.is_file() || file_type.is_symlink())
                .then(|| entry.path().strip_prefix(&root).ok().map(text))
                .flatten()
        })
        .collect()
}

pub(super) type Viewed = (String, Vec<(usize, usize)>, Option<Vec<u8>>);

pub(super) fn view(root: &Path, path: &str) -> Result<Viewed> {
    let full = root.join(path);
    let bytes = std::fs::read(&full)?;
    if crate::tools::image_type(&bytes).is_some() {
        return Ok((String::new(), Vec::new(), Some(bytes)));
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let added = diff::hunks(root, path).unwrap_or_default();
    Ok((text, added, None))
}

pub(super) fn relative(root: &Path, event: &NotifyEvent) -> Vec<String> {
    let renamed = matches!(&event.kind, EventKind::Modify(ModifyKind::Name(_)));
    event.paths[event.paths.len().saturating_sub(1) * usize::from(renamed)..]
        .iter()
        .filter_map(|path| path.strip_prefix(root).ok().map(text))
        .filter(|path| !path.is_empty())
        .collect()
}

pub(super) fn add<T: PartialEq>(rows: &mut Vec<T>, row: T) {
    if !rows.contains(&row) {
        rows.push(row);
    }
}

pub(super) fn text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(super) fn unignored(root: &Path, mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths.dedup();
    let mut listed = std::collections::HashMap::new();
    paths.retain(|path| {
        let full = root.join(path);
        let dir = full.parent().unwrap_or(root).to_path_buf();
        let shown = listed.entry(dir).or_insert_with_key(|dir| {
            walk(dir, Some(1))
                .map(ignore::DirEntry::into_path)
                .collect::<Vec<_>>()
        });
        !path.split('/').any(|part| part.starts_with('.'))
            && (!full.exists() || shown.contains(&full))
    });
    paths
}

pub(super) fn clipboard() -> Result<Vec<u8>> {
    let out = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", "get the clipboard as «class PNGf»"])
        .stderr(std::process::Stdio::null())
        .output()?;
    if out.status.success() {
        return png(&String::from_utf8_lossy(&out.stdout));
    }
    let out = std::process::Command::new("/usr/bin/pbpaste")
        .stderr(std::process::Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(Error::Tool(format!("pbpaste failed: {}", out.status)));
    }
    Ok(out.stdout)
}

fn png(reply: &str) -> Result<Vec<u8>> {
    let hex = reply
        .trim()
        .strip_prefix("«data PNGf")
        .and_then(|rest| rest.strip_suffix('»'))
        .filter(|hex| hex.len() % 2 == 0)
        .ok_or_else(|| Error::Tool("clipboard image reply is malformed".to_string()))?;
    (0..hex.len())
        .step_by(2)
        .map(|at| {
            hex.get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| Error::Tool(format!("clipboard image has bad hex at {at}")))
        })
        .collect()
}
