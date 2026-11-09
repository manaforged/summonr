use std::path::{Path, PathBuf};

use super::ToolRegistry;
use crate::durable::store::{self, Staged};
use summonr_core::error::{Error, Result};

const HUNK_LINE_CHARS: usize = 160;

pub(in crate::tools) struct Change {
    pub(in crate::tools) dest: PathBuf,
    pub(in crate::tools) display: String,
    pub(in crate::tools) action: Action,
}

pub(in crate::tools) enum Action {
    Write(String),
    Delete,
    Move { to: PathBuf, display: String },
}

impl Change {
    pub(in crate::tools) fn write(dest: PathBuf, display: &str, content: String) -> Self {
        Self {
            dest,
            display: display.to_string(),
            action: Action::Write(content),
        }
    }

    pub(in crate::tools) fn content(&self) -> Option<&str> {
        match &self.action {
            Action::Write(content) => Some(content),
            Action::Delete | Action::Move { .. } => None,
        }
    }
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
    pub(in crate::tools) fn apply_changes(&self, changes: &[Change]) -> Result<()> {
        let staged = changes
            .iter()
            .map(|change| {
                change
                    .content()
                    .map(|content| stage(&change.dest, &change.display, content))
                    .transpose()
            })
            .collect::<Result<Vec<_>>>()?;
        for (index, (change, staged)) in changes.iter().zip(staged).enumerate() {
            let applied = match (staged, &change.action) {
                (Some(staged), _) => staged.commit(),
                (None, Action::Move { to, .. }) => store::rename(&change.dest, to),
                (None, _) => std::fs::remove_file(&change.dest),
            };
            if let Err(error) = applied {
                let message = match &change.action {
                    Action::Move { display, .. } => {
                        format!("cannot move `{}` to `{display}`: {error}", change.display)
                    }
                    Action::Write(_) | Action::Delete => {
                        format!("cannot change `{}`: {error}", change.display)
                    }
                };
                if index == 0 {
                    return Err(Error::Tool(message));
                }
                let done: Vec<&str> = changes[..index]
                    .iter()
                    .map(|change| change.display.as_str())
                    .collect();
                return Err(Error::Tool(format!(
                    "{message}; already applied: {}",
                    done.join(", ")
                )));
            }
            self.note_wrote(&change.display);
            if let Action::Move { display, .. } = &change.action {
                self.note_wrote(display);
            }
        }
        Ok(())
    }
}

pub(in crate::tools) fn patch_of(changes: &[Change]) -> Option<String> {
    let patch: Vec<String> = changes
        .iter()
        .enumerate()
        // complexity: bounded by the files in one tool call
        .filter_map(|(index, change)| change_patch(&changes[..index], change))
        .collect();
    (!patch.is_empty()).then(|| patch.join("\n"))
}

fn change_patch(earlier: &[Change], change: &Change) -> Option<String> {
    let path = &change.display;
    let after = match &change.action {
        Action::Move { display, .. } => {
            return Some(format!("--- a/{path}\n+++ b/{display}"));
        }
        Action::Write(content) => content.as_str(),
        Action::Delete => "",
    };
    let before = match staged(earlier, &change.dest) {
        Some(content) => content.unwrap_or_default().to_string(),
        None => current(&change.dest, path).unwrap_or_default(),
    };
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let hunks = super::spans::patch(&old, &new)?;
    let side = |lines: &[&str], prefix: &str| {
        if lines.is_empty() {
            "/dev/null".to_string()
        } else {
            format!("{prefix}/{path}")
        }
    };
    Some(format!(
        "--- {}\n+++ {}\n{hunks}",
        side(&old, "a"),
        side(&new, "b")
    ))
}

pub(in crate::tools) fn staged<'a>(changes: &'a [Change], dest: &Path) -> Option<Option<&'a str>> {
    changes
        .iter()
        .rev()
        .find(|change| change.dest == dest)
        .map(Change::content)
}

pub(in crate::tools) fn preview(
    before: &str,
    content: &str,
    view: impl Into<View>,
) -> (String, Option<String>) {
    edit_view(before, content, view.into())
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
    let limit = crate::limits::limits().edit_hunk_rows;
    let mut lines = lines
        .iter()
        .map(|line| crate::display::ellipsize(line, HUNK_LINE_CHARS))
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
