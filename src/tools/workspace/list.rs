use std::path::Path;

use summonr_core::error::{Error, Result};
use summonr_core::walk::{self, Walk};

const LIST_MAX: usize = 200;

pub(super) fn format_tree(path: &str, dir: &Path, depth: usize, base: Walk) -> Result<String> {
    let mut rows = Vec::new();
    let walk = Walk {
        depth: Some(depth),
        ..base
    };
    for entry in walk.entries(dir).skip(1) {
        let rel = entry
            .path()
            .strip_prefix(dir)
            .unwrap_or_else(|_| entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let is_dir = walk::is_dir(&entry);
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
            "\n… {} more: tree incomplete; narrow with a smaller path or depth, or use search",
            total - LIST_MAX
        ));
    }
    Ok(out)
}

pub(super) fn format_directory(path: &str, dir: &Path, base: Walk) -> Result<String> {
    std::fs::read_dir(dir)
        .map_err(|error| Error::Tool(format!("cannot list `{path}`: {error}")))?;
    let mut entries = Vec::new();
    let walk = Walk {
        depth: Some(1),
        ..base
    };
    for entry in walk.entries(dir).skip(1) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = entry.metadata().ok();
        let kind = if meta.as_ref().is_some_and(std::fs::Metadata::is_dir) {
            "dir"
        } else {
            "file"
        };
        let size = meta
            .filter(|meta| meta.is_file())
            .map(|meta| format!("{} bytes", meta.len()))
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
            "\n… {omit} more: listing incomplete; use recursive true or a narrower path",
            omit = total - LIST_MAX
        ));
    }
    Ok(out)
}
