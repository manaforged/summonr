use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::Entry;
use crate::durable::store;
use crate::error::Result;

const FILE: &str = "score.jsonl";
const HOME_DIR: &str = "score";

pub(crate) fn path(workspace: &Path) -> Result<PathBuf> {
    if let Some(dir) = crate::diff::state_dir(workspace) {
        return Ok(dir.join(FILE));
    }
    let slug = crate::mail::slug(&workspace.display().to_string());
    Ok(crate::prompt::summonr_home()?
        .join(HOME_DIR)
        .join(format!("{slug}.jsonl")))
}

pub(crate) fn read(path: &Path) -> Result<Vec<Entry>> {
    let text = store::read_or_empty(path)?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let mut entries = Vec::with_capacity(lines.len());
    for (index, line) in lines.iter().enumerate() {
        match serde_json::from_str::<Entry>(line) {
            Ok(entry) => entries.push(entry),
            Err(_) if index + 1 == lines.len() && !text.ends_with('\n') => {}
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: line {}: {error}", path.display(), index + 1),
                )
                .into());
            }
        }
    }
    Ok(entries)
}

pub(crate) fn append(path: &Path, entry: &Entry) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let held = store::lock(path, true)?;
    let mut bytes = serde_json::to_vec(entry)?;
    bytes.push(b'\n');
    let torn = std::fs::read(path)
        .ok()
        .is_some_and(|old| !old.is_empty() && old.last() != Some(&b'\n'));
    if torn {
        bytes.insert(0, b'\n');
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    drop(held);
    Ok(())
}
