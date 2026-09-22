use std::path::Path;

use crate::SKIP_DIRS;
use crate::error::{Error, Result};

const LIST_MAX: usize = 200;

pub(super) fn format_tree(path: &str, dir: &Path, depth: usize) -> Result<String> {
    let mut rows = Vec::new();
    for entry in ignore::WalkBuilder::new(dir)
        .require_git(false)
        .hidden(false)
        .max_depth(Some(depth))
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
        })
        .build()
        .filter_map(std::result::Result::ok)
        .skip(1)
    {
        let rel = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or_else(|_| entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
        rows.push((
            entry.depth(),
            format!("{rel}{}", if is_dir { "/" } else { "" }),
        ));
    }
    rows.sort();
    let total = rows.len();
    rows.truncate(LIST_MAX);
    rows.sort_by(|left, right| left.1.cmp(&right.1));
    let mut out = format!("{path}  {total} entries");
    for (_, row) in rows {
        out.push('\n');
        out.push_str(&row);
    }
    if total > LIST_MAX {
        out.push_str(&format!(
            "\n… {} more — tree incomplete; narrow with a smaller path or depth, or use op=search",
            total - LIST_MAX
        ));
    }
    Ok(out)
}

pub(super) fn format_directory(path: &str, dir: &Path) -> Result<String> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)
        .map_err(|error| Error::Tool(format!("cannot list `{path}`: {error}")))?
    {
        let entry = entry.map_err(|error| Error::Tool(format!("cannot list `{path}`: {error}")))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        let meta = entry.metadata().ok();
        let kind = if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
            "dir"
        } else {
            "file"
        };
        let size = meta
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len().to_string())
            .unwrap_or_default();
        entries.push((name, kind, size));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let total = entries.len();
    entries.truncate(LIST_MAX);
    let mut out = format!("{path}  {total} entries");
    for (name, kind, size) in entries {
        out.push('\n');
        out.push_str(&name);
        if kind == "dir" {
            out.push('/');
        }
        out.push('\t');
        out.push_str(kind);
        if !size.is_empty() {
            out.push('\t');
            out.push_str(&size);
        }
    }
    if total > LIST_MAX {
        out.push_str(&format!(
            "\n… {omit} more — listing incomplete; use recursive true or a narrower path",
            omit = total - LIST_MAX
        ));
    }
    Ok(out)
}
