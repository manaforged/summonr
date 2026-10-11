use std::hash::Hasher;

use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

const IRREGULAR: &[(&str, &str)] = &[("child", "children")];

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
    value: &'a str,
    items: Vec<&'a str>,
}

pub(crate) struct Frontmatter<'a> {
    fields: Vec<Field<'a>>,
    pub(crate) body: &'a str,
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
        for line in rest.split_inclusive('\n') {
            offset += line.len();
            let line = line.trim();
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
                fields.push(Field {
                    key: key.trim().trim_matches(QUOTES),
                    value: value.trim().trim_matches(QUOTES),
                    items: Vec::new(),
                });
            }
        }
        plain
    }

    pub(crate) fn get(&self, key: &str) -> Option<&'a str> {
        self.fields
            .iter()
            .find(|field| field.key == key && !field.value.is_empty())
            .map(|field| field.value)
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
