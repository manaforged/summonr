use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::hashline::{resolve_span, tagged_span};
use summonr_core::error::{Error, Result};
use summonr_core::text::squash;

use super::edit::Patch;

mod closest;

use closest::no_match;

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct EditOp {
    #[serde(deserialize_with = "present")]
    tag: Option<Value>,
    #[serde(deserialize_with = "present")]
    old: Option<Value>,
    #[serde(deserialize_with = "present")]
    new: Option<Value>,
    #[serde(deserialize_with = "present")]
    text: Option<Value>,
    #[serde(deserialize_with = "present")]
    end_tag: Option<Value>,
    #[serde(deserialize_with = "present")]
    op: Option<Value>,
    #[serde(deserialize_with = "present")]
    replace_all: Option<Value>,
    #[serde(deserialize_with = "present")]
    all: Option<Value>,
    #[serde(deserialize_with = "present")]
    replace_text: Option<Value>,
    #[serde(deserialize_with = "present")]
    replace: Option<Value>,
    #[serde(deserialize_with = "present")]
    insert_after: Option<Value>,
    #[serde(deserialize_with = "present")]
    delete: Option<Value>,
    #[serde(skip)]
    keys: Vec<String>,
}

fn present<'de, D: Deserializer<'de>>(input: D) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(input).map(Some)
}

fn string<'a>(field: &'a Option<Value>, name: &str) -> Result<&'a str> {
    text_of(field).ok_or_else(|| Error::Tool(format!("{name} is required")))
}

fn text_of(field: &Option<Value>) -> Option<&str> {
    field.as_ref().and_then(Value::as_str)
}

impl EditOp {
    pub(super) fn parse(op: &Value) -> Self {
        let mut parsed: Self = serde_json::from_value(op.clone()).unwrap_or_default();
        parsed.keys = op
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default();
        parsed
    }

    pub(super) fn patch(&self, text: &str) -> Result<Patch> {
        if let Some(Value::String(tag)) = &self.replace_text {
            return self.replace_text(text, Some(tag));
        }
        if self.old.is_some() && self.new.is_some() {
            return self.replace_text(text, self.optional_tag()?);
        }
        if let Some(tag) = self.optional_tag()? {
            let end_tag = text_of(&self.end_tag);
            if self.new.is_some() {
                return replace_patch(text, tag, end_tag, lines(string(&self.new, "new")?));
            }
            if self.text.is_some() {
                return insert_patch(text, tag, string(&self.text, "text")?);
            }
            if text_of(&self.op) == Some("delete") {
                return replace_patch(text, tag, end_tag, Vec::new());
            }
        }
        if let Some(replace) = &self.replace {
            let inner = nested(
                replace,
                "replace must be an object with tag, new, and optional end_tag; use replace_text with old and new for exact text",
            )?;
            let new = inner
                .new
                .as_ref()
                .or(inner.text.as_ref())
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Tool("replace requires new".to_string()))?;
            let tag = string(&inner.tag, "tag")?;
            return replace_patch(text, tag, text_of(&inner.end_tag), lines(new));
        }
        if let Some(replace) = &self.replace_text {
            let inner = nested(
                replace,
                "replace_text must be an object with old, new, and optional tag",
            )?;
            return inner.replace_text(text, inner.optional_tag()?);
        }
        if let Some(insert) = &self.insert_after {
            let inner = nested(insert, "insert_after must be an object with tag and text")?;
            return insert_patch(
                text,
                string(&inner.tag, "tag")?,
                string(&inner.text, "text")?,
            );
        }
        if let Some(delete) = &self.delete {
            let inner = nested(
                delete,
                "delete must be an object with tag and optional end_tag",
            )?;
            let tag = string(&inner.tag, "tag")?;
            return replace_patch(text, tag, text_of(&inner.end_tag), Vec::new());
        }
        Err(Error::Tool(format!(
            "edit got keys [{}]; it takes old+new, tag+new, tag+text, op=delete, or edits",
            self.keys.join(", ")
        )))
    }

    pub(super) fn context(&self, error: Error, text: &str) -> Error {
        let Error::Tool(message) = error else {
            return error;
        };
        let Some(Ok((line, _))) = text_of(&self.tag).map(|tag| resolve_span(text, tag, None))
        else {
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

    fn replace_text(&self, text: &str, tag: Option<&str>) -> Result<Patch> {
        let all = self
            .replace_all
            .as_ref()
            .or(self.all.as_ref())
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let (old, new) = (string(&self.old, "old")?, string(&self.new, "new")?);
        replace_text_patch(text, tag, old, new, all)
    }

    fn optional_tag(&self) -> Result<Option<&str>> {
        match &self.tag {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(tag)) if tag.trim().is_empty() => Ok(None),
            Some(Value::String(tag)) => Ok(Some(tag)),
            Some(_) => Err(Error::Tool("tag must be a non-empty string".to_string())),
        }
    }
}

fn nested(value: &Value, shape: &str) -> Result<EditOp> {
    if !value.is_object() {
        return Err(Error::Tool(shape.to_string()));
    }
    Ok(EditOp::parse(value))
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
            Some(tag) => tagged_match(text, tag, old)?,
            None => {
                if global_matches == 0 {
                    if let Some(patch) = relaxed_patch(text, old, new) {
                        return Ok(patch);
                    }
                    return Err(no_match(text, old));
                }
                return Err(ambiguous(text, old, global_matches));
            }
        }
    };
    Ok(span_patch(text, old, new, byte, line_index))
}

fn tagged_match(text: &str, tag: &str, old: &str) -> Result<(usize, usize)> {
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
    Ok((matches[0].0, line_index))
}

fn ambiguous(text: &str, old: &str, global_matches: usize) -> Error {
    let mut places = text
        .match_indices(old)
        .map(|(byte, _)| text[..byte].bytes().filter(|byte| *byte == b'\n').count() + 1);
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
    Error::Tool(format!(
        "old occurs {global_matches} times; pass tag to anchor one edit, or replace_all true to change them all. Matches start on lines {}{more}",
        lines_list.join(", ")
    ))
}

fn span_patch(text: &str, old: &str, new: &str, byte: usize, line_index: usize) -> Patch {
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
    Patch {
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
    }
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

fn replace_patch(
    text: &str,
    tag: &str,
    end_tag: Option<&str>,
    replacement: Vec<String>,
) -> Result<Patch> {
    let (start, end) = resolve_span(text, tag, end_tag)?;
    Ok(Patch {
        start,
        end: end + 1,
        anchor_start: start,
        anchor_end: end + 1,
        replacement,
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

pub(super) fn lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    text.split('\n').map(str::to_string).collect()
}
