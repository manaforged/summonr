use std::hash::Hasher;

use crate::error::{Error, Result};
use crate::text::Fnv64;

const ALPH: &[u8] = b"0123456789abcdefghijklmnopqrstuv";

#[must_use]
pub(crate) fn line_hash(text: impl AsRef<[u8]>) -> String {
    let mut hasher = Fnv64::default();
    hasher.write(text.as_ref());
    let hash = hasher.finish();
    let mut out = [b'0'; 4];
    let mut rest = hash;
    for slot in out.iter_mut().rev() {
        let index = usize::try_from(rest & 31).unwrap_or(0);
        *slot = ALPH.get(index).copied().unwrap_or(b'0');
        rest >>= 5;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Clone, Copy)]
pub(crate) enum Tags<'a> {
    Off,
    FromText,
    Given(&'a [String]),
}

#[must_use]
pub(crate) fn tagged_lines<S: AsRef<str>>(
    lines: &[S],
    first: usize,
    total: usize,
    tags: Tags<'_>,
) -> String {
    if total == 0 || lines.is_empty() {
        return format!("lines 0-0/{total}");
    }
    let last = first + lines.len() - 1;
    let mut out = format!("lines {}-{}/{}", first + 1, last + 1, total);
    for (offset, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        out.push('\n');
        out.push_str(&(first + offset + 1).to_string());
        out.push(':');
        let hash = match tags {
            Tags::Off => None,
            Tags::FromText => Some(line_hash(line)),
            Tags::Given(hashes) => hashes.get(offset).cloned(),
        };
        if let Some(hash) = hash {
            out.push_str(&hash);
            out.push('|');
        }
        out.push_str(line);
    }
    out
}

#[must_use]
pub(crate) fn tagged_span(lines: &[&str], first: usize, last: usize) -> String {
    let total = lines.len();
    if total == 0 {
        return "lines 0-0/0".to_string();
    }
    let first = first.min(total - 1);
    let last = last.min(total - 1).max(first);
    tagged_lines(&lines[first..=last], first, total, Tags::FromText)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tag {
    Line { number: usize, hash: String },
    Hash(String),
}

fn parse_tag(tag: &str) -> Result<Tag> {
    let tag = tag.trim();
    let tag = tag.split_once('|').map_or(tag, |(anchor, _)| anchor.trim());
    if tag.is_empty() {
        return Err(Error::Tool("edit tag is empty".to_string()));
    }
    if let Some((left, right)) = tag.split_once(':')
        && let Ok(number) = left.parse::<usize>()
        && number > 0
        && is_hash(right)
    {
        return Ok(Tag::Line {
            number,
            hash: right.to_string(),
        });
    }
    if is_hash(tag) {
        return Ok(Tag::Hash(tag.to_string()));
    }
    Err(Error::Tool(format!(
        "`{tag}` is not a tag; pass N:hash exactly as read_file printed it"
    )))
}

fn is_hash(value: &str) -> bool {
    (2..=8).contains(&value.len())
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'v'))
}

fn hits(content: &str, parsed: &Tag) -> Vec<usize> {
    content
        .lines()
        .enumerate()
        .filter(|(index, line)| {
            let hash = line_hash(line);
            match parsed {
                Tag::Line { number, hash: want } => index + 1 == *number && hash == *want,
                Tag::Hash(want) => hash == *want,
            }
        })
        .map(|(index, _)| index)
        .collect()
}

fn stale(content: &str, tag: &str, parsed: &Tag) -> Error {
    let near = match parsed {
        Tag::Line { number, .. } => Some(*number),
        Tag::Hash(_) => None,
    };
    match near {
        Some(number) if content.lines().count() > 0 => {
            let first = number.saturating_sub(4).saturating_sub(1);
            Error::Tool(format!(
                "stale tag `{tag}`: that line changed. Current lines there:\n{}",
                tagged_span(&content.lines().collect::<Vec<_>>(), first, number + 3)
            ))
        }
        _ => Error::Tool(format!(
            "stale or unknown tag `{tag}`; the line changed; use a tag from the latest edit result or read_file"
        )),
    }
}

fn ambiguous(content: &str, tag: &str, found: &[usize]) -> Error {
    let lines: Vec<&str> = content.lines().collect();
    let options: Vec<String> = found
        .iter()
        .take(6)
        .map(|index| {
            let line = lines.get(*index).copied().unwrap_or("");
            format!("{}:{}|{}", index + 1, line_hash(line), line)
        })
        .collect();
    Error::Tool(format!(
        "tag `{tag}` matches {} lines: use one of:\n{}",
        found.len(),
        options.join("\n")
    ))
}

fn locate(content: &str, tag: &str) -> Result<usize> {
    let parsed = parse_tag(tag)?;
    let found = hits(content, &parsed);
    match found.as_slice() {
        [only] => Ok(*only),
        [] => Err(stale(content, tag, &parsed)),
        _ => Err(ambiguous(content, tag, &found)),
    }
}

fn span(content: &str, tag: &str, end_tag: Option<&str>) -> Result<(usize, usize)> {
    let end_tag = end_tag.filter(|end| !end.trim().is_empty());
    let Some(end_tag) = end_tag else {
        let start = locate(content, tag)?;
        return Ok((start, start));
    };
    let start = locate(content, tag)?;
    let end = locate(content, end_tag)?;
    if end < start {
        return Err(Error::Tool(
            "end_tag is before tag; check the tags in the latest edit result".to_string(),
        ));
    }
    Ok((start, end))
}

pub(crate) fn resolve_span(
    content: &str,
    tag: &str,
    end_tag: Option<&str>,
) -> Result<(usize, usize)> {
    span(content, tag, end_tag)
}

#[cfg(test)]
#[path = "hashline_tests.rs"]
mod tests;
