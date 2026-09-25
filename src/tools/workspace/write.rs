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
        tags: bool,
    ) -> Result<String> {
        let before = current(dest, display)?;
        let preview = self.stage_write(dest, &before, content, tags)?;
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
        tags: bool,
    ) -> Result<String> {
        if let Some((option, detail)) =
            crate::prefs::banned_edit(dest, before, content, &self.allowed_edits)
        {
            return Err(Error::EditApproval {
                setting: option,
                detail,
            });
        }
        Ok(edit_view(before, content, tags))
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

fn edit_view(before: &str, after: &str, tags: bool) -> String {
    if before.is_empty() && !after.is_empty() {
        return format!("new file, {} lines", after.lines().count());
    }
    let hunk = unified_hunk(before, after);
    if !tags {
        return hunk;
    }
    let (first, last) = crate::hashline::dirty_span(before, after);
    let span = crate::hashline::tagged_span(after, first.saturating_sub(1), last.saturating_add(1));
    format!(
        "{}\n{hunk}",
        cap_rows(span.lines().map(str::to_string).collect()).join("\n")
    )
}

fn cap_rows(lines: Vec<String>) -> Vec<String> {
    let limit = super::read::READ_DEFAULT_LIMIT;
    let mut lines = lines
        .iter()
        .map(|line| crate::child::bounded(line, 160))
        .collect::<Vec<_>>();
    if lines.len() > limit {
        let skipped = lines.len() - limit;
        lines.truncate(limit);
        lines.push(format!("[{skipped} lines clipped]"));
    }
    lines
}

pub(super) fn unified_hunk(before: &str, after: &str) -> String {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let mut first = 0;
    while first < old.len() && first < new.len() && old[first] == new[first] {
        first += 1;
    }
    if first == old.len() && first == new.len() {
        return "no change".to_string();
    }
    let (mut old_end, mut new_end) = (old.len(), new.len());
    while old_end > first && new_end > first && old[old_end - 1] == new[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    let start = first.saturating_sub(1);
    let after_end = (new_end + 1).min(new.len());
    let mut body = Vec::new();
    body.extend(old[start..first].iter().map(|line| format!(" {line}")));
    body.extend(old[first..old_end].iter().map(|line| format!("-{line}")));
    body.extend(new[first..new_end].iter().map(|line| format!("+{line}")));
    body.extend(
        new[new_end..after_end]
            .iter()
            .map(|line| format!(" {line}")),
    );
    let body = cap_rows(body);
    let limit = super::read::READ_DEFAULT_LIMIT;
    let (kept, note) = body.split_at(body.len().min(limit));
    let old_count = kept.iter().filter(|line| !line.starts_with('+')).count();
    let new_count = kept.iter().filter(|line| !line.starts_with('-')).count();
    let side = |count: usize| {
        if count == 0 {
            format!("{start},0")
        } else {
            format!("{},{count}", start + 1)
        }
    };
    let mut out = String::new();
    if let Some(note) = note.first() {
        out.push_str(note);
        out.push('\n');
    }
    out.push_str(&format!("@@ -{} +{} @@", side(old_count), side(new_count)));
    for line in kept {
        out.push('\n');
        out.push_str(line);
    }
    out
}

fn stage<'a>(dest: &'a Path, display: &str, content: &str) -> Result<Staged<'a>> {
    store::stage(dest, content.as_bytes(), true)
        .map_err(|error| Error::Tool(format!("cannot write `{display}`: {error}")))
}
