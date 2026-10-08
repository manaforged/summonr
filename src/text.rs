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

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
