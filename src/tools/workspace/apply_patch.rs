use summonr_core::error::{Error, Result};

use super::edit::{Patch, apply_patches, restore, terminators};
use super::ops::replace_text_patch;
use super::write::{Action, Change, staged};
use crate::tools::{ToolOutput, ToolRegistry};
use summonr_core::text::squash;

enum Section {
    Update {
        path: String,
        to: Option<String>,
        hunks: Vec<Hunk>,
    },
    Add {
        path: String,
        lines: Vec<String>,
    },
    Delete {
        path: String,
    },
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
        let (change, text) = section.stage(registry, &changes)?;
        out.push_str(&text);
        changes.push(change);
        if let Section::Update {
            path, to: Some(to), ..
        } = section
        {
            changes.push(registry.plan_move(path, to)?);
            out.push_str(&format!("moved {path} -> {to}\n"));
        }
    }
    for (index, change) in changes.iter().enumerate() {
        if let Action::Move { to, .. } = &change.action
            && changes.iter().enumerate().any(|(other_index, other)| {
                other_index != index
                    && (other.dest == *to
                        || matches!(&other.action, Action::Move { to: target, .. } if target == to))
            })
        {
            return Err(Error::Tool(format!(
                "multiple changes target move destination `{}`",
                to.display()
            )));
        }
    }
    let diff = super::write::patch_of(&changes);
    registry.apply_changes(&changes)?;
    if let Some(dest) = changes
        .iter()
        .rev()
        .find_map(|change| match &change.action {
            Action::Move { to, .. } => Some(to),
            Action::Write(_) => Some(&change.dest),
            Action::Delete => None,
        })
        && let Some(check) = registry.check_after_edit(dest).await
    {
        out.push_str(&check.text);
    }
    Ok(ToolOutput::mutated(out.trim_end().to_string(), diff))
}

impl Section {
    fn stage(&self, registry: &ToolRegistry, changes: &[Change]) -> Result<(Change, String)> {
        match self {
            Self::Update { path, hunks, .. } => stage_update(registry, changes, path, hunks),
            Self::Add { path, lines } => stage_add(registry, changes, path, lines),
            Self::Delete { path } => {
                let dest = registry.resolve_existing(path)?;
                registry.check_write(&dest)?;
                let change = Change {
                    dest,
                    display: path.clone(),
                    action: Action::Delete,
                };
                Ok((change, format!("deleted {path}\n")))
            }
        }
    }
}

fn stage_update(
    registry: &ToolRegistry,
    changes: &[Change],
    path: &str,
    hunks: &[Hunk],
) -> Result<(Change, String)> {
    let dest = registry.resolve_existing(path)?;
    registry.check_write(&dest)?;
    let raw = match staged(changes, &dest) {
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
    let preview = super::write::preview(&raw, &edited, false).0;
    let change = Change::write(dest, path, edited);
    Ok((change, format!("edited {path}\n{preview}\n")))
}

fn stage_add(
    registry: &ToolRegistry,
    changes: &[Change],
    path: &str,
    lines: &[String],
) -> Result<(Change, String)> {
    let dest = registry.resolve_write(path)?;
    registry.check_write(&dest)?;
    if staged(changes, &dest).map_or_else(|| dest.exists(), |content| content.is_some()) {
        return Err(Error::Tool(format!(
            "*** Add File: `{path}` already exists; use Update File"
        )));
    }
    let content = format!("{}\n", lines.join("\n"));
    let preview = super::write::preview("", &content, false).0;
    let change = Change::write(dest, path, content);
    Ok((change, format!("added {path}\n{preview}\n")))
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
    Update {
        path: String,
        to: Option<String>,
        hunks: Vec<Hunk>,
    },
    Add {
        path: String,
        lines: Vec<String>,
    },
}

enum Header {
    Update,
    Add,
    Delete,
}

#[derive(Default)]
struct Parser {
    sections: Vec<Section>,
    pending: Option<Pending>,
    hunk: Option<Hunk>,
}

impl Parser {
    fn flush_hunk(&mut self) {
        if let Some(done) = self.hunk.take()
            && let Some(Pending::Update { hunks, .. }) = self.pending.as_mut()
        {
            hunks.push(done);
        }
    }

    fn flush_pending(&mut self) {
        match self.pending.take() {
            Some(Pending::Update { path, to, hunks }) => {
                self.sections.push(Section::Update { path, to, hunks });
            }
            Some(Pending::Add { path, lines }) => {
                self.sections.push(Section::Add { path, lines });
            }
            None => {}
        }
    }

    fn header(&mut self, kind: &Header, path: &str) {
        self.flush_hunk();
        self.flush_pending();
        match kind {
            Header::Update => {
                self.pending = Some(Pending::Update {
                    path: path.to_string(),
                    to: None,
                    hunks: Vec::new(),
                });
            }
            Header::Add => {
                self.pending = Some(Pending::Add {
                    path: path.to_string(),
                    lines: Vec::new(),
                });
            }
            Header::Delete => self.sections.push(Section::Delete {
                path: path.to_string(),
            }),
        }
    }

    fn open_hunk(&mut self, anchor: &str) {
        self.flush_hunk();
        let anchor = anchor.trim();
        self.hunk = Some(Hunk {
            anchor: (!anchor.is_empty()).then(|| anchor.to_string()),
            old: Vec::new(),
            new: Vec::new(),
            eof: false,
        });
    }

    fn line(&mut self, line: &str) -> Result<()> {
        if line == "*** Begin Patch" || line == "*** End Patch" {
            return Ok(());
        }
        if let Some((kind, path)) = header(line) {
            self.header(kind, path);
            return Ok(());
        }
        if let Some(to) = line.strip_prefix("*** Move to: ") {
            match &mut self.pending {
                Some(Pending::Update {
                    to: target, hunks, ..
                }) if target.is_none()
                    && hunks.is_empty()
                    && self.hunk.is_none()
                    && !to.trim().is_empty() =>
                {
                    *target = Some(to.trim().to_string());
                    return Ok(());
                }
                Some(Pending::Update { .. } | Pending::Add { .. }) | None => {
                    return Err(Error::Tool(
                        "Move to must appear once, immediately after an Update File header"
                            .to_string(),
                    ));
                }
            }
        }
        if line == "*** End of File" {
            if let Some(open) = self.hunk.as_mut() {
                open.eof = true;
            }
            return Ok(());
        }
        if let Some(anchor) = line.strip_prefix("@@") {
            self.open_hunk(anchor);
            return Ok(());
        }
        self.body(line)
    }

    fn body(&mut self, line: &str) -> Result<()> {
        match self.pending.as_mut() {
            Some(Pending::Add { lines, .. }) => {
                lines.push(line.strip_prefix('+').unwrap_or(line).to_string());
            }
            Some(Pending::Update { .. }) => {
                let Some(open) = self.hunk.as_mut() else {
                    return Err(Error::Tool(format!(
                        "patch line before the first @@ context: `{line}`"
                    )));
                };
                open.push(line);
            }
            None => {
                return Err(Error::Tool(format!(
                    "patch line outside a file section: `{line}`"
                )));
            }
        }
        Ok(())
    }
}

impl Hunk {
    fn push(&mut self, line: &str) {
        match line.as_bytes().first() {
            Some(b'-') => self.old.push(line[1..].to_string()),
            Some(b'+') => self.new.push(line[1..].to_string()),
            Some(_) | None => {
                let text = line.strip_prefix(' ').unwrap_or(line);
                self.old.push(text.to_string());
                self.new.push(text.to_string());
            }
        }
    }
}

fn header(line: &str) -> Option<(&'static Header, &str)> {
    static HEADERS: [(&str, Header); 3] = [
        ("Update File", Header::Update),
        ("Add File", Header::Add),
        ("Delete File", Header::Delete),
    ];
    HEADERS.iter().find_map(|(kind, tag)| {
        line.strip_prefix(&format!("*** {kind}:"))
            .map(str::trim)
            .filter(|path| !path.is_empty())
            .map(|path| (tag, path))
    })
}

pub(crate) fn patch_files(patch: &str) -> Vec<&str> {
    patch
        .lines()
        .filter_map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            header(line)
                .map(|(_, path)| path)
                .or_else(|| line.strip_prefix("*** Move to: ").map(str::trim))
        })
        .collect()
}

fn parse(patch: &str) -> Result<Vec<Section>> {
    let mut parser = Parser::default();
    for raw in patch.lines() {
        parser.line(raw.strip_suffix('\r').unwrap_or(raw))?;
    }
    parser.flush_hunk();
    parser.flush_pending();
    Ok(parser.sections)
}
