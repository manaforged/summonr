use std::borrow::Cow;
use std::hash::Hasher;
use std::iter::Peekable;
use std::str::SplitInclusive;

use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

const IRREGULAR: &[(&str, &str)] = &[("child", "children"), ("entry", "entries")];

pub(crate) fn plural(count: usize, word: &str) -> String {
    if count == 1 {
        return format!("{count} {word}");
    }
    if let Some((_, many)) = IRREGULAR.iter().find(|(one, _)| *one == word) {
        return format!("{count} {many}");
    }
    if word.ends_with(['s', 'x']) || word.ends_with("ch") || word.ends_with("sh") {
        format!("{count} {word}es")
    } else {
        format!("{count} {word}s")
    }
}

#[must_use]
pub(crate) fn megabytes(kb: u64) -> String {
    format!("{:.0} MB", kb as f64 / 1024.0)
}

pub(crate) fn duration(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let (hours, minutes, seconds) = (secs / 3600, secs / 60 % 60, secs % 60);
    if hours > 0 {
        format!("{hours}h{minutes:02}m{seconds:02}s")
    } else {
        format!("{minutes}m{seconds:02}s")
    }
}

pub(crate) fn ellipsize(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let end = text.grapheme_indices(true).find_map(|(index, grapheme)| {
        used += cell_width(grapheme);
        (used > width.saturating_sub(1)).then_some(index)
    });
    let mut shown = text[..end.unwrap_or(text.len())].to_string();
    shown.push('…');
    shown
}

pub(crate) fn ellipsize_start(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let start = text
        .grapheme_indices(true)
        .rev()
        .find_map(|(index, grapheme)| {
            used += cell_width(grapheme);
            (used > width.saturating_sub(1)).then_some(index + grapheme.len())
        });
    format!("…{}", &text[start.unwrap_or(0)..])
}

pub(crate) struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv64 {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

pub(crate) fn fnv64(bytes: &[u8]) -> String {
    let mut hasher = Fnv64::default();
    hasher.write(bytes);
    format!("{:016x}", hasher.finish())
}

pub(crate) fn redact<'a>(text: &str, secrets: impl IntoIterator<Item = &'a str>) -> String {
    secrets
        .into_iter()
        .filter(|secret| !secret.is_empty())
        .fold(text.to_string(), |text, secret| {
            text.replace(secret, "[secret]")
        })
}

const KEY_ECHOES: &[&str] = &[
    "api key provided:",
    "key provided:",
    "api key:",
    "x-api-key:",
];

pub(crate) fn hide_key_echo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, marker)) = KEY_ECHOES
        .iter()
        .filter_map(|marker| {
            rest.to_ascii_lowercase()
                .find(marker)
                .map(|at| (at, marker.len()))
        })
        .min_by_key(|(at, _)| *at)
    {
        let after = at + marker;
        out.push_str(&rest[..after]);
        let tail = &rest[after..];
        let gap = tail.len() - tail.trim_start().len();
        let token = tail[gap..]
            .find(|ch: char| ch.is_whitespace() || ch == '"' || ch == ',')
            .unwrap_or(tail.len() - gap);
        let token = token - usize::from(tail[gap..gap + token].ends_with('.'));
        out.push_str(&tail[..gap]);
        out.push_str("[key]");
        rest = &tail[gap + token..];
    }
    out.push_str(rest);
    out
}

#[must_use]
pub(crate) fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[must_use]
pub(crate) fn lines_within(text: &str, width: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match parts.last_mut() {
            Some(last) if cell_width(last) + 1 + cell_width(word) <= width => {
                last.push(' ');
                last.push_str(word);
            }
            Some(_) | None => parts.push(ellipsize(word, width)),
        }
    }
    parts
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;

pub(crate) const FENCE: &str = "---";

const ITEM: &str = "- ";
const QUOTES: [char; 2] = ['"', '\''];

struct Field<'a> {
    key: &'a str,
    value: Cow<'a, str>,
    items: Vec<&'a str>,
}

pub(crate) struct Frontmatter<'a> {
    fields: Vec<Field<'a>>,
    pub(crate) body: &'a str,
}

#[derive(Clone, Copy)]
enum ScalarStyle {
    Folded,
    Literal,
}

#[derive(Clone, Copy)]
enum Chomping {
    Strip,
    Clip,
    Keep,
}

const BLOCK_SCALARS: &[(&str, ScalarStyle, Chomping)] = &[
    (">", ScalarStyle::Folded, Chomping::Clip),
    (">-", ScalarStyle::Folded, Chomping::Strip),
    (">+", ScalarStyle::Folded, Chomping::Keep),
    ("|", ScalarStyle::Literal, Chomping::Clip),
    ("|-", ScalarStyle::Literal, Chomping::Strip),
    ("|+", ScalarStyle::Literal, Chomping::Keep),
];

fn block_scalar(
    lines: &mut Peekable<SplitInclusive<'_, char>>,
    offset: &mut usize,
    parent: usize,
    style: ScalarStyle,
    chomping: Chomping,
) -> Option<String> {
    let mut body = Vec::new();
    while let Some(line) = lines.next_if(|line| {
        line.trim().is_empty() || line.len() - line.trim_start_matches(' ').len() > parent
    }) {
        *offset += line.len();
        body.push(line.trim_end_matches(['\r', '\n']));
    }
    let indent = body
        .iter()
        .find(|line| !line.trim().is_empty())
        .map_or_else(
            || body.iter().map(|line| line.len()).max().unwrap_or(0),
            |line| line.len() - line.trim_start_matches(' ').len(),
        );
    let prefix = " ".repeat(indent);
    for line in &mut body {
        let raw = *line;
        *line = raw
            .strip_prefix(&prefix)
            .or_else(|| raw.trim().is_empty().then_some(""))?;
    }
    let end = body
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |at| at + 1);
    let mut value = String::new();
    for (at, line) in body[..end].iter().enumerate() {
        value.push_str(line);
        value.push_str(match (style, body[..end].get(at + 1).copied()) {
            (ScalarStyle::Literal, _) | (_, None) => "\n",
            (ScalarStyle::Folded, Some(next))
                if line.is_empty()
                    || line.starts_with([' ', '\t'])
                    || next.starts_with([' ', '\t']) =>
            {
                "\n"
            }
            (ScalarStyle::Folded, Some("")) => "",
            (ScalarStyle::Folded, Some(_)) => " ",
        });
    }
    match chomping {
        Chomping::Strip => value.truncate(value.trim_end_matches('\n').len()),
        Chomping::Clip => {}
        Chomping::Keep => value.push_str(&"\n".repeat(body.len() - end)),
    }
    Some(value)
}

impl<'a> Frontmatter<'a> {
    pub(crate) fn parse(text: &'a str) -> Self {
        let plain = Self {
            fields: Vec::new(),
            body: text,
        };
        let Some(rest) = text.trim_start().strip_prefix(FENCE).and_then(|rest| {
            rest.strip_prefix('\n')
                .or_else(|| rest.strip_prefix("\r\n"))
        }) else {
            return plain;
        };
        let mut fields: Vec<Field<'a>> = Vec::new();
        let mut offset = 0;
        let mut lines = rest.split_inclusive('\n').peekable();
        while let Some(raw) = lines.next() {
            offset += raw.len();
            let line = raw.trim();
            if line == FENCE {
                return Self {
                    fields,
                    body: &rest[offset..],
                };
            }
            if let Some(item) = line.strip_prefix(ITEM) {
                if let Some(field) = fields.last_mut().filter(|field| field.value.is_empty()) {
                    field.items.push(item.trim().trim_matches(QUOTES));
                }
            } else if let Some((key, value)) = line.split_once(':') {
                let block = BLOCK_SCALARS
                    .iter()
                    .find(|(marker, ..)| *marker == value.trim());
                let value = if let Some((_, style, chomping)) = block {
                    let parent = raw.len() - raw.trim_start_matches(' ').len();
                    match block_scalar(&mut lines, &mut offset, parent, *style, *chomping) {
                        Some(value) => Cow::Owned(value),
                        None => Cow::Borrowed(value.trim()),
                    }
                } else {
                    Cow::Borrowed(value.trim().trim_matches(QUOTES))
                };
                fields.push(Field {
                    key: key.trim().trim_matches(QUOTES),
                    value,
                    items: Vec::new(),
                });
            }
        }
        plain
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.key == key && !field.value.is_empty())
            .map(|field| field.value.as_ref())
    }

    pub(crate) fn list(&self, key: &str) -> Option<Vec<String>> {
        let field = self.fields.iter().find(|field| field.key == key)?;
        if field.value.is_empty() {
            return Some(field.items.iter().map(|item| (*item).to_string()).collect());
        }
        let inline = field.value.trim_start_matches('[').trim_end_matches(']');
        Some(
            crate::policy::unquoted(inline, &[","])
                .iter()
                .map(|item| item.trim().trim_matches(QUOTES))
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect(),
        )
    }
}
