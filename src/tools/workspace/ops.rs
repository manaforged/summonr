use serde_json::Value;

use crate::error::{Error, Result};
use crate::hashline::resolve_span;

use super::edit::Patch;
use super::object_string;

const OP_REPLACE_TEXT: &str = "replace_text";
const OP_INSERT_AFTER: &str = "insert_after";

pub(super) fn parse_patch(text: &str, op: &Value) -> Result<Patch> {
    if let Some(tag) = op.get(OP_REPLACE_TEXT).and_then(Value::as_str) {
        return replace_text_patch(
            text,
            Some(tag),
            object_string(op, "old")?,
            object_string(op, "new")?,
            replace_all(op),
        );
    }
    if op.get("old").is_some() && op.get("new").is_some() {
        return replace_text_patch(
            text,
            optional_tag(op)?,
            object_string(op, "old")?,
            object_string(op, "new")?,
            replace_all(op),
        );
    }
    if op.get("tag").is_some() {
        let tag = object_string(op, "tag")?;
        if op.get("new").is_some() {
            return replace_patch(
                text,
                tag,
                op.get("end_tag").and_then(Value::as_str),
                object_string(op, "new")?,
            );
        }
        if op.get("text").is_some() {
            return insert_patch(text, tag, object_string(op, "text")?);
        }
        if op.get("op").and_then(Value::as_str) == Some("delete") {
            return delete_patch(text, tag, op.get("end_tag").and_then(Value::as_str));
        }
    }
    if let Some(replace) = op.get("replace") {
        if !replace.is_object() {
            return Err(Error::Tool(
                "replace must be an object with tag, new, and optional end_tag; use replace_text with old and new for exact text"
                    .to_string(),
            ));
        }
        let tag = object_string(replace, "tag")?;
        let new = replace
            .get("new")
            .or_else(|| replace.get("text"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Tool("replace requires new".to_string()))?;
        let end = replace.get("end_tag").and_then(Value::as_str);
        return replace_patch(text, tag, end, new);
    }
    if let Some(replace) = op.get(OP_REPLACE_TEXT) {
        if !replace.is_object() {
            return Err(Error::Tool(
                "replace_text must be an object with old, new, and optional tag".to_string(),
            ));
        }
        return replace_text_patch(
            text,
            optional_tag(replace)?,
            object_string(replace, "old")?,
            object_string(replace, "new")?,
            replace_all(replace),
        );
    }
    if let Some(insert) = op.get(OP_INSERT_AFTER) {
        if !insert.is_object() {
            return Err(Error::Tool(
                "insert_after must be an object with tag and text".to_string(),
            ));
        }
        let tag = object_string(insert, "tag")?;
        let body = object_string(insert, "text")?;
        return insert_patch(text, tag, body);
    }
    if let Some(delete) = op.get("delete") {
        if !delete.is_object() {
            return Err(Error::Tool(
                "delete must be an object with tag and optional end_tag".to_string(),
            ));
        }
        let tag = object_string(delete, "tag")?;
        let end = delete.get("end_tag").and_then(Value::as_str);
        return delete_patch(text, tag, end);
    }
    Err(Error::Tool(
        "edit requires old+new, tag+new, tag+text, op=delete, or edits".to_string(),
    ))
}

fn optional_tag(value: &Value) -> Result<Option<&str>> {
    match value.get("tag") {
        None => Ok(None),
        Some(Value::String(tag)) if !tag.trim().is_empty() => Ok(Some(tag)),
        Some(_) => Err(Error::Tool("tag must be a non-empty string".to_string())),
    }
}

fn replace_all(value: &Value) -> bool {
    value
        .get("replace_all")
        .or_else(|| value.get("all"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(super) fn replace_text_patch(
    text: &str,
    tag: Option<&str>,
    old: &str,
    new: &str,
    all: bool,
) -> Result<Patch> {
    if old.is_empty() {
        return Err(Error::Tool("old text is empty".to_string()));
    }
    let global_matches = count_matches(text, old);
    if all {
        if global_matches == 0 {
            return Err(no_match(text, old));
        }
        return Ok(Patch {
            start: 0,
            end: usize::MAX,
            anchor_start: 0,
            anchor_end: usize::MAX,
            replacement: lines(&text.replace(old, new)),
            relaxed: false,
        });
    }
    let (byte, line_index) = if global_matches == 1 {
        let byte = text.find(old).ok_or_else(|| {
            Error::Tool("replace_text.old disappeared while resolving the edit".to_string())
        })?;
        (
            byte,
            text[..byte].bytes().filter(|byte| *byte == b'\n').count(),
        )
    } else {
        match tag {
            Some(tag) => {
                let line_index = resolve_span(text, tag, None)
                    .or_else(|error| relocate_exact_anchor(text, tag).ok_or(error))?
                    .0;
                let line_start = line_byte(text, line_index);
                let line_end = text[line_start..]
                    .find('\n')
                    .map_or(text.len(), |offset| line_start + offset);
                let matches = text
                    .match_indices(old)
                    .filter(|(byte, _)| *byte >= line_start && *byte <= line_end)
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(Error::Tool(format!(
                        "replace_text.old must start exactly once on the anchored line; found {}",
                        matches.len()
                    )));
                }
                (matches[0].0, line_index)
            }
            None => {
                if global_matches == 0 {
                    if let Some(patch) = relaxed_patch(text, old, new) {
                        return Ok(patch);
                    }
                    return Err(no_match(text, old));
                }
                let mut places = text.match_indices(old).map(|(byte, _)| {
                    text[..byte].bytes().filter(|byte| *byte == b'\n').count() + 1
                });
                let mut lines_list = Vec::new();
                for _ in 0..10 {
                    let Some(line) = places.next() else { break };
                    lines_list.push(line.to_string());
                }
                let more = if global_matches > 10 {
                    format!(" and {} more", global_matches - 10)
                } else {
                    String::new()
                };
                return Err(Error::Tool(format!(
                    "old occurs {global_matches} times — pass tag to anchor one edit, or replace_all true to change them all. Matches start on lines {}{more}",
                    lines_list.join(", ")
                )));
            }
        }
    };
    let end_byte = byte + old.len();
    let end_line = text.as_bytes()[..end_byte]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count();
    let start_byte = line_byte(text, line_index);
    let line_end = text[end_byte..]
        .find('\n')
        .map_or(text.len(), |offset| end_byte + offset);
    let suffix = &text[end_byte..line_end];
    let replacement = format!("{}{new}{suffix}", &text[start_byte..byte]);
    Ok(Patch {
        start: line_index,
        end: end_line + 1,
        anchor_start: line_index,
        anchor_end: end_line + 1,
        replacement: if replacement.is_empty() && !old.ends_with('\n') {
            vec![String::new()]
        } else {
            lines(&replacement)
        },
        relaxed: false,
    })
}

fn relaxed_patch(text: &str, old: &str, new: &str) -> Option<Patch> {
    let target: Vec<String> = lines(old).iter().map(|line| squash(line)).collect();
    let rows: Vec<String> = lines(text).iter().map(|line| squash(line)).collect();
    let len = target.len();
    if len == 0 || target.iter().all(String::is_empty) || len > rows.len() {
        return None;
    }
    let mut found = (0..=rows.len() - len).filter(|index| rows[*index..*index + len] == target[..]);
    let start = found.next()?;
    if found.next().is_some() {
        return None;
    }
    Some(Patch {
        start,
        end: start + len,
        anchor_start: start,
        anchor_end: start + len,
        replacement: lines(new),
        relaxed: true,
    })
}

pub(super) fn squash(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn relocate_exact_anchor(text: &str, tag: &str) -> Option<(usize, usize)> {
    let anchor = tag.split_once('|').map_or(tag, |(anchor, _)| anchor.trim());
    let (number, hash) = anchor.split_once(':')?;
    if number.parse::<usize>().ok()? == 0 {
        return None;
    }
    resolve_span(text, hash, None).ok()
}

fn line_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line - 1)
        .map_or(0, |(byte, _)| byte + 1)
}

fn count_matches(text: &str, needle: &str) -> usize {
    text.match_indices(needle).count()
}

fn no_match(text: &str, old: &str) -> Error {
    let mut out = "old does not appear in the file".to_string();
    let needle = old
        .lines()
        .map(str::trim)
        .filter(|line| line.len() >= 4)
        .max_by_key(|line| line.len());
    if let Some(needle) = needle {
        let squashed = needle.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut scored: Vec<(f64, usize, &str)> = text
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let candidate = line.split_whitespace().collect::<Vec<_>>().join(" ");
                if candidate.is_empty() {
                    return None;
                }
                let score = lcs(&squashed, &candidate) as f64 / squashed.len() as f64;
                (score >= 0.5).then_some((score, index, line))
            })
            .collect();
        scored.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.1.cmp(&right.1))
        });
        let found: Vec<String> = scored
            .iter()
            .take(5)
            .map(|(_, index, line)| format!("  {}: {}", index + 1, line.trim()))
            .collect();
        if !found.is_empty() {
            out.push_str(&format!("; closest lines:\n{}", found.join("\n")));
        }
    }
    Error::Tool(out)
}

fn lcs(left: &str, right: &str) -> usize {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let mut row = vec![0_usize; right.len() + 1];
    for &a in left {
        let mut diagonal = 0;
        for index in 1..=right.len() {
            let above = row[index];
            row[index] = if a == right[index - 1] {
                diagonal + 1
            } else {
                above.max(row[index - 1])
            };
            diagonal = above;
        }
    }
    row[right.len()]
}

fn replace_patch(text: &str, tag: &str, end_tag: Option<&str>, new: &str) -> Result<Patch> {
    let (start, end) = resolve_span(text, tag, end_tag)?;
    Ok(Patch {
        start,
        end: end + 1,
        anchor_start: start,
        anchor_end: end + 1,
        replacement: lines(new),
        relaxed: false,
    })
}

fn insert_patch(text: &str, tag: &str, body: &str) -> Result<Patch> {
    if body.is_empty() {
        return Err(Error::Tool("insert_after.text is empty".to_string()));
    }
    let (anchor, _) = resolve_span(text, tag, None)?;
    Ok(Patch {
        start: anchor + 1,
        end: anchor + 1,
        anchor_start: anchor,
        anchor_end: anchor + 1,
        replacement: lines(body),
        relaxed: false,
    })
}

fn delete_patch(text: &str, tag: &str, end_tag: Option<&str>) -> Result<Patch> {
    let (start, end) = resolve_span(text, tag, end_tag)?;
    Ok(Patch {
        start,
        end: end + 1,
        anchor_start: start,
        anchor_end: end + 1,
        replacement: Vec::new(),
        relaxed: false,
    })
}

pub(super) fn lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    text.split('\n').map(str::to_string).collect()
}
