use serde_json::Value;

use crate::error::{Error, Result};
use crate::hashline::{resolve_span, tagged_span};

use super::ops::{lines, parse_patch};

pub(super) fn move_target(args: &Value) -> Option<&str> {
    args.get("to")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|to| !to.is_empty())
}

pub(super) fn uses_tags(args: &Value) -> bool {
    match args {
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key == "tag" || uses_tags(value)),
        Value::Array(items) => items.iter().any(uses_tags),
        _ => false,
    }
}

pub(super) fn edit_error_context(error: Error, text: &str, args: &Value) -> Error {
    let Error::Tool(message) = error else {
        return error;
    };
    let Some(tag) = args.get("tag").and_then(Value::as_str) else {
        return Error::Tool(message);
    };
    let Ok((line, _)) = resolve_span(text, tag, None) else {
        return Error::Tool(message);
    };
    Error::Tool(format!(
        "{message}\nCurrent lines:\n{}",
        tagged_span(
            &text.lines().collect::<Vec<_>>(),
            line.saturating_sub(2),
            line.saturating_add(2)
        )
    ))
}

pub(super) fn apply_one(text: &str, op: &Value) -> Result<(String, bool)> {
    apply_patches(text, vec![parse_patch(text, op)?])
}

pub(super) struct Patch {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) anchor_start: usize,
    pub(super) anchor_end: usize,
    pub(super) replacement: Vec<String>,
    pub(super) relaxed: bool,
}

pub(super) fn apply_many(original: &str, ops: &[Value]) -> Result<(String, bool)> {
    let mut patches = Vec::with_capacity(ops.len());
    for (index, op) in ops.iter().enumerate() {
        let patch = parse_patch(original, op).map_err(|error| {
            match edit_error_context(error, original, op) {
                Error::Tool(message) => Error::Tool(format!("edit {}: {message}", index + 1)),
                other => other,
            }
        })?;
        patches.push(patch);
    }
    if overlaps(&mut patches) {
        return apply_sequence(original, ops);
    }
    apply_patches(original, patches)
}

fn overlaps(patches: &mut [Patch]) -> bool {
    patches.sort_by_key(|patch| patch.anchor_start);
    patches
        .windows(2)
        .any(|pair| pair[1].anchor_start < pair[0].anchor_end)
}

pub(super) fn apply_sequence(original: &str, ops: &[Value]) -> Result<(String, bool)> {
    let mut edited = original.to_string();
    let mut relaxed = false;
    for (index, op) in ops.iter().enumerate() {
        let patch = parse_patch(&edited, op).map_err(|error| {
            match edit_error_context(error, &edited, op) {
                Error::Tool(message) => Error::Tool(format!("edit {}: {message}", index + 1)),
                other => other,
            }
        })?;
        let (text, patch_relaxed) = apply_patches(&edited, vec![patch])?;
        relaxed |= patch_relaxed;
        edited = text;
    }
    Ok((edited, relaxed))
}

pub(super) fn apply_patches(original: &str, mut patches: Vec<Patch>) -> Result<(String, bool)> {
    let relaxed = patches.iter().any(|patch| patch.relaxed);
    if overlaps(&mut patches) {
        return Err(Error::Tool(
            "edits overlap — merge them into one replace".to_string(),
        ));
    }
    patches.sort_by_key(|patch| (patch.start, patch.start != patch.end));
    let mut lines = lines(original);
    for patch in patches.into_iter().rev() {
        let end = patch.end.min(lines.len());
        let start = patch.start.min(end);
        lines.splice(start..end, patch.replacement);
    }
    let mut text = lines.join("\n");
    if original.ends_with('\n') && !text.ends_with('\n') {
        text.push('\n');
    }
    Ok((text, relaxed))
}

pub(super) fn terminators(raw: &str) -> Vec<&'static str> {
    let mut endings = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        match rest.find('\n') {
            Some(at) => {
                let crlf = at > 0 && rest.as_bytes()[at - 1] == b'\r';
                endings.push(if crlf { "\r\n" } else { "\n" });
                rest = &rest[at + 1..];
            }
            None => {
                endings.push("");
                rest = "";
            }
        }
    }
    endings
}

pub(super) fn restore(endings: &[&'static str], original: &str, edited: &str) -> String {
    let crlf = endings.iter().filter(|e| **e == "\r\n").count();
    let lf = endings.iter().filter(|e| **e == "\n").count();
    let majority = if crlf > lf { "\r\n" } else { "\n" };
    let old: Vec<&str> = original.lines().collect();
    let new: Vec<&str> = edited.lines().collect();
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let trailing = edited.ends_with('\n');
    let mut out = String::with_capacity(edited.len() + new.len());
    for (index, line) in new.iter().enumerate() {
        let kept = if index < prefix {
            endings.get(index).copied()
        } else if index >= new.len() - suffix {
            endings.get(old.len() - (new.len() - index)).copied()
        } else {
            None
        };
        out.push_str(line);
        let last = index + 1 == new.len();
        if last && !trailing {
            break;
        }
        out.push_str(match kept {
            Some(ending) if !ending.is_empty() => ending,
            _ => majority,
        });
    }
    out
}
