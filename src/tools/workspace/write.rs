use std::path::{Path, PathBuf};

use super::ToolRegistry;
use crate::durable::store::{self, Staged};
use crate::error::{Error, Result};

const HUNK_ROWS: usize = crate::limits::EDIT_HUNK_ROWS;
const HUNK_LINE_CHARS: usize = 160;

pub(in crate::tools) struct Change {
    pub(in crate::tools) dest: PathBuf,
    pub(in crate::tools) display: String,
    pub(in crate::tools) content: Option<String>,
}

pub(in crate::tools) struct Written {
    pub(in crate::tools) preview: String,
    pub(in crate::tools) patch: Option<String>,
}

#[derive(Clone, Copy)]
pub(in crate::tools) enum View {
    Hunk,
    Tagged,
    Overwrite,
}

impl From<bool> for View {
    fn from(tags: bool) -> Self {
        if tags { View::Tagged } else { View::Hunk }
    }
}

impl ToolRegistry {
    pub(in crate::tools) fn atomic_write(
        &self,
        dest: &Path,
        display: &str,
        before: &str,
        content: &str,
        view: impl Into<View>,
    ) -> Result<Written> {
        let (preview, patch) = self.stage_write_full(dest, before, content, view)?;
        stage(dest, display, content)?
            .commit()
            .map_err(|error| Error::Tool(format!("cannot replace `{display}`: {error}")))?;
        Ok(Written { preview, patch })
    }

    pub(in crate::tools) fn stage_write(
        &self,
        dest: &Path,
        before: &str,
        content: &str,
        view: impl Into<View>,
    ) -> Result<String> {
        Ok(self.stage_write_full(dest, before, content, view)?.0)
    }

    fn stage_write_full(
        &self,
        dest: &Path,
        before: &str,
        content: &str,
        view: impl Into<View>,
    ) -> Result<(String, Option<String>)> {
        if let Some((option, detail)) =
            crate::prefs::banned_edit(dest, before, content, &self.allowed_edits)
        {
            return Err(Error::EditApproval {
                setting: option,
                detail,
            });
        }
        Ok(edit_view(before, content, view.into()))
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

fn edit_view(before: &str, after: &str, view: View) -> (String, Option<String>) {
    if matches!(view, View::Overwrite) {
        let old: Vec<&str> = before.lines().collect();
        let new: Vec<&str> = after.lines().collect();
        return (overwritten(before, after), super::spans::patch(&old, &new));
    }
    if before.is_empty() && !after.is_empty() {
        let new: Vec<&str> = after.lines().collect();
        return (
            format!("new file, {} lines", new.len()),
            super::spans::patch(&[], &new),
        );
    }
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let (spans, patch) = super::spans::changed_and_patch(&old, &new);
    let summary = super::spans::summary(old.len(), new.len(), &spans);
    let text = if !matches!(view, View::Tagged) || spans.is_empty() {
        summary
    } else {
        tagged_view(&new, &spans, summary)
    };
    (text, patch)
}

fn tagged_view(new: &[&str], spans: &[super::spans::Span], summary: String) -> String {
    let tagged = spans
        .iter()
        .flat_map(|span| {
            crate::hashline::tagged_span(
                new,
                span.start.saturating_sub(1),
                span.start + span.len.max(1),
            )
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
        })
        .collect();
    format!("{}\n{summary}", cap_rows(tagged).join("\n"))
}

fn overwritten(before: &str, after: &str) -> String {
    if before == after {
        "no change".to_string()
    } else if before.is_empty() {
        "new file".to_string()
    } else {
        format!("replaced {} lines", before.lines().count())
    }
}

fn cap_rows(lines: Vec<String>) -> Vec<String> {
    let limit = HUNK_ROWS;
    let mut lines = lines
        .iter()
        .map(|line| crate::text::ellipsize(line, HUNK_LINE_CHARS))
        .collect::<Vec<_>>();
    if lines.len() > limit {
        let skipped = lines.len() - limit;
        lines.truncate(limit);
        lines.push(format!("[{skipped} lines clipped]"));
    }
    lines
}

fn stage<'a>(dest: &'a Path, display: &str, content: &str) -> Result<Staged<'a>> {
    store::stage(dest, content.as_bytes(), true)
        .map_err(|error| Error::Tool(format!("cannot write `{display}`: {error}")))
}
