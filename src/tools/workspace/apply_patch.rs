use std::path::Path;

use crate::error::{Error, Result};

use super::edit::{Patch, apply_patches, restore, terminators};
use super::ops::{replace_text_patch, squash};
use super::write::{Change, apply_changes};
use crate::tools::{ToolOutput, ToolRegistry};

enum Section {
    Update { path: String, hunks: Vec<Hunk> },
    Add { path: String, lines: Vec<String> },
    Delete { path: String },
}

struct Hunk {
    anchor: Option<String>,
    old: Vec<String>,
    new: Vec<String>,
    eof: bool,
}

pub(super) async fn run(registry: &ToolRegistry, patch: &str) -> Result<ToolOutput> {
    let sections = parse(patch)?;
    if sections.is_empty() {
        return Err(Error::Tool(
            "patch has no *** Update File:, *** Add File:, or *** Delete File: sections"
                .to_string(),
        ));
    }
    let mut changes: Vec<Change> = Vec::new();
    let mut out = String::new();
    for section in &sections {
        match section {
            Section::Update { path, hunks } => {
                let dest = registry.resolve_existing(path)?;
                registry.check_write(&dest)?;
                let raw = match staged(&changes, &dest) {
                    Some(Some(content)) => content.to_string(),
                    Some(None) => {
                        return Err(Error::Tool(format!(
                            "`{path}` is deleted earlier in this patch"
                        )));
                    }
                    None => std::fs::read_to_string(&dest)
                        .map_err(|error| Error::Tool(format!("cannot read `{path}`: {error}")))?,
                };
                let endings = terminators(&raw);
                let mut text = raw.replace("\r\n", "\n");
                for hunk in hunks {
                    let patch = hunk_patch(&text, hunk).map_err(|error| match error {
                        Error::Tool(message) => Error::Tool(format!("{path}: {message}")),
                        other => other,
                    })?;
                    text = apply_patches(&text, vec![patch])?.0;
                }
                let edited = restore(&endings, &raw, &text);
                let preview = registry.stage_write(&dest, &raw, &edited, false)?;
                out.push_str(&format!("edited {path}\n{preview}\n"));
                changes.push(Change {
                    dest,
                    display: path.clone(),
                    content: Some(edited),
                });
            }
            Section::Add { path, lines } => {
                let dest = registry.resolve_write(path)?;
                registry.check_write(&dest)?;
                if staged(&changes, &dest)
                    .map_or_else(|| dest.exists(), |content| content.is_some())
                {
                    return Err(Error::Tool(format!(
                        "*** Add File: `{path}` already exists; use Update File"
                    )));
                }
                let content = format!("{}\n", lines.join("\n"));
                let preview = registry.stage_write(&dest, "", &content, false)?;
                out.push_str(&format!("added {path}\n{preview}\n"));
                changes.push(Change {
                    dest,
                    display: path.clone(),
                    content: Some(content),
                });
            }
            Section::Delete { path } => {
                let dest = registry.resolve_existing(path)?;
                registry.check_write(&dest)?;
                out.push_str(&format!("deleted {path}\n"));
                changes.push(Change {
                    dest,
                    display: path.clone(),
                    content: None,
                });
            }
        }
    }
    apply_changes(&changes)?;
    if let Some(change) = changes.iter().rev().find(|change| change.content.is_some())
        && let Some(check) = registry.check_after_edit(&change.dest).await
    {
        out.push_str(&check.text);
    }
    Ok(ToolOutput {
        text: out.trim_end().to_string(),
        success: true,
        mutated: true,
        grant: None,
        images: Vec::new(),
        patch: None,
    })
}

fn staged<'a>(changes: &'a [Change], dest: &Path) -> Option<Option<&'a str>> {
    changes
        .iter()
        .rev()
        .find(|change| change.dest == dest)
        .map(|change| change.content.as_deref())
}

fn hunk_patch(text: &str, hunk: &Hunk) -> Result<Patch> {
    if hunk.eof {
        let total = text.lines().count();
        return Ok(Patch {
            start: total,
            end: total,
            anchor_start: total.saturating_sub(1),
            anchor_end: total,
            replacement: hunk.new.clone(),
            relaxed: false,
        });
    }
    if hunk.old.is_empty() {
        let anchor = hunk.anchor.as_deref().ok_or_else(|| {
            Error::Tool("hunk adds lines but has no context or @@ anchor".to_string())
        })?;
        let wanted = squash(anchor);
        let found: Vec<usize> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| squash(line) == wanted)
            .map(|(index, _)| index)
            .collect();
        let [line] = found.as_slice() else {
            return Err(Error::Tool(format!(
                "@@ anchor `{anchor}` matches {} lines; add context lines to pin it",
                found.len()
            )));
        };
        return Ok(Patch {
            start: line + 1,
            end: line + 1,
            anchor_start: *line,
            anchor_end: line + 1,
            replacement: hunk.new.clone(),
            relaxed: false,
        });
    }
    replace_text_patch(
        text,
        None,
        &hunk.old.join("\n"),
        &hunk.new.join("\n"),
        false,
    )
}

enum Pending {
    Update { path: String, hunks: Vec<Hunk> },
    Add { path: String, lines: Vec<String> },
}

fn parse(patch: &str) -> Result<Vec<Section>> {
    let mut sections: Vec<Section> = Vec::new();
    let mut pending: Option<Pending> = None;
    let mut hunk: Option<Hunk> = None;
    macro_rules! flush_hunk {
        () => {
            if let Some(done) = hunk.take()
                && let Some(Pending::Update { hunks, .. }) = pending.as_mut()
            {
                hunks.push(done);
            }
        };
    }
    macro_rules! flush_pending {
        () => {
            match pending.take() {
                Some(Pending::Update { path, hunks }) => {
                    sections.push(Section::Update { path, hunks });
                }
                Some(Pending::Add { path, lines }) => {
                    sections.push(Section::Add { path, lines });
                }
                None => {}
            }
        };
    }
    for raw in patch.lines() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line == "*** Begin Patch" || line == "*** End Patch" {
            continue;
        }
        enum Header {
            Update,
            Add,
            Delete,
        }
        let header = [
            ("Update File", Header::Update),
            ("Add File", Header::Add),
            ("Delete File", Header::Delete),
        ]
        .iter()
        .find_map(|(kind, tag)| {
            line.strip_prefix(&format!("*** {kind}:"))
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(|path| (tag, path))
        });
        if let Some((kind, path)) = header {
            flush_hunk!();
            flush_pending!();
            match kind {
                Header::Update => {
                    pending = Some(Pending::Update {
                        path: path.to_string(),
                        hunks: Vec::new(),
                    });
                }
                Header::Add => {
                    pending = Some(Pending::Add {
                        path: path.to_string(),
                        lines: Vec::new(),
                    });
                }
                Header::Delete => sections.push(Section::Delete {
                    path: path.to_string(),
                }),
            }
            continue;
        }
        if line == "*** End of File" {
            if let Some(open) = hunk.as_mut() {
                open.eof = true;
            }
            continue;
        }
        if let Some(anchor) = line.strip_prefix("@@") {
            flush_hunk!();
            let anchor = anchor.trim();
            hunk = Some(Hunk {
                anchor: (!anchor.is_empty()).then(|| anchor.to_string()),
                old: Vec::new(),
                new: Vec::new(),
                eof: false,
            });
            continue;
        }
        match pending.as_mut() {
            Some(Pending::Add { lines, .. }) => {
                lines.push(line.strip_prefix('+').unwrap_or(line).to_string());
            }
            Some(Pending::Update { .. }) => {
                let Some(open) = hunk.as_mut() else {
                    return Err(Error::Tool(format!(
                        "patch line before the first @@ context: `{line}`"
                    )));
                };
                match line.as_bytes().first() {
                    Some(b'-') => open.old.push(line[1..].to_string()),
                    Some(b'+') => open.new.push(line[1..].to_string()),
                    Some(_) | None => {
                        let text = line.strip_prefix(' ').unwrap_or(line);
                        open.old.push(text.to_string());
                        open.new.push(text.to_string());
                    }
                }
            }
            None => {
                return Err(Error::Tool(format!(
                    "patch line outside a file section: `{line}`"
                )));
            }
        }
    }
    flush_hunk!();
    flush_pending!();
    Ok(sections)
}
