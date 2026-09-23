use std::path::{Path, PathBuf};

use super::ToolRegistry;
use crate::durable::store::{self, Staged};
use crate::error::{Error, Result};

pub(in crate::tools) struct Change {
    pub(in crate::tools) dest: PathBuf,
    pub(in crate::tools) display: String,
    pub(in crate::tools) content: Option<String>,
}

impl ToolRegistry {
    pub(in crate::tools) fn atomic_write(
        &self,
        dest: &Path,
        display: &str,
        content: &str,
    ) -> Result<String> {
        let before = current(dest, display)?;
        let preview = self.stage_write(dest, &before, content)?;
        stage(dest, display, content)?
            .commit()
            .map_err(|error| Error::Tool(format!("cannot replace `{display}`: {error}")))?;
        Ok(preview)
    }

    pub(in crate::tools) fn stage_write(
        &self,
        dest: &Path,
        before: &str,
        content: &str,
    ) -> Result<String> {
        if let Some((option, detail)) =
            crate::prefs::banned_edit(dest, before, content, &self.allowed_edits)
        {
            return Err(Error::EditApproval {
                setting: option,
                detail,
            });
        }
        Ok(edit_view(before, content))
    }

    pub(crate) fn allow_edit(&mut self, setting: &'static str) {
        if !self.allowed_edits.contains(&setting) {
            self.allowed_edits.push(setting);
        }
    }

    pub(crate) fn reset_edits(&mut self) {
        self.allowed_edits.clear();
    }
}

pub(in crate::tools) fn current(dest: &Path, display: &str) -> Result<String> {
    match std::fs::read(dest) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| {
            Error::Tool(format!(
                "`{display}` is not UTF-8 text ({error}); change it with run_command"
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(Error::Tool(format!("cannot read `{display}`: {error}"))),
    }
}

pub(in crate::tools) fn apply_changes(changes: &[Change]) -> Result<()> {
    let staged = changes
        .iter()
        .map(|change| {
            change
                .content
                .as_ref()
                .map(|content| stage(&change.dest, &change.display, content))
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    let mut done: Vec<&str> = Vec::new();
    for (change, staged) in changes.iter().zip(staged) {
        let applied = match staged {
            Some(staged) => staged.commit(),
            None => std::fs::remove_file(&change.dest),
        };
        if let Err(error) = applied {
            let message = format!("cannot change `{}`: {error}", change.display);
            if done.is_empty() {
                return Err(Error::Tool(message));
            }
            return Err(Error::Tool(format!(
                "{message}; already applied: {}",
                done.join(", ")
            )));
        }
        done.push(&change.display);
    }
    Ok(())
}

fn edit_view(before: &str, after: &str) -> String {
    if before.is_empty() && !after.is_empty() {
        return format!("new file, {} lines", after.lines().count());
    }
    let (first, last) = crate::hashline::dirty_span(before, after);
    let mut lines =
        crate::hashline::tagged_span(after, first.saturating_sub(1), last.saturating_add(1))
            .lines()
            .map(|line| crate::child::bounded(line, 160))
            .collect::<Vec<_>>();
    if lines.len() > super::read::READ_DEFAULT_LIMIT {
        let skipped = lines.len() - super::read::READ_DEFAULT_LIMIT;
        lines.truncate(super::read::READ_DEFAULT_LIMIT);
        lines.push(format!("[{skipped} lines clipped]"));
    }
    lines.join("\n")
}

fn stage<'a>(dest: &'a Path, display: &str, content: &str) -> Result<Staged<'a>> {
    store::stage(dest, content.as_bytes(), true)
        .map_err(|error| Error::Tool(format!("cannot write `{display}`: {error}")))
}
