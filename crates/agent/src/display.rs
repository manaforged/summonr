use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Measures how many terminal cells a string takes.
pub(crate) type Width = fn(&str) -> usize;

pub enum Span {
    Cells(usize),
    Bytes(usize),
}

impl From<usize> for Span {
    fn from(cells: usize) -> Self {
        Self::Cells(cells)
    }
}

/// The cell width of a one-line summary: a session preview, a step title, or a tool call argument.
pub(crate) const SUMMARY_WIDTH: usize = 72;

pub(crate) fn ellipsize(text: &str, span: impl Into<Span>) -> String {
    ellipsize_by(UnicodeWidthStr::width, text, span)
}

pub(crate) fn ellipsize_start(text: &str, width: usize) -> String {
    ellipsize_start_by(UnicodeWidthStr::width, text, width)
}

pub fn ellipsize_by(cell_width: Width, text: &str, span: impl Into<Span>) -> String {
    let width = match span.into() {
        Span::Cells(width) => width,
        Span::Bytes(limit) if text.len() <= limit => return text.to_string(),
        Span::Bytes(limit) => return format!("{}…", &text[..text.floor_char_boundary(limit)]),
    };
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

pub fn ellipsize_start_by(cell_width: Width, text: &str, width: usize) -> String {
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

#[must_use]
pub fn lines_within_by(cell_width: Width, text: &str, width: usize) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match parts.last_mut() {
            Some(last) if cell_width(last) + 1 + cell_width(word) <= width => {
                last.push(' ');
                last.push_str(word);
            }
            Some(_) | None => parts.push(ellipsize_by(cell_width, word, width)),
        }
    }
    parts
}

const QUIET_OPS: [&str; 2] = ["file", "sequence"];
const LISTING_OPS: [&str; 1] = ["list"];

pub fn tool_arg(arguments: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments) else {
        return String::new();
    };
    let field = |name: &str| value.get(name).and_then(serde_json::Value::as_str);
    if field("pattern").is_some() || field("glob").is_some() {
        let parts = [
            field("pattern").map(|pattern| format!("\"{pattern}\"")),
            field("glob").map(str::to_string),
            field("path").map(|path| format!("in {path}")),
        ];
        return parts.into_iter().flatten().collect::<Vec<_>>().join(" ");
    }
    let paths = value
        .get("paths")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        });
    let picked = ["path", "command", "name", "query", "url", "child"]
        .iter()
        .find_map(|name| field(name))
        .map(str::to_string)
        .or(paths)
        .or_else(|| field("patch").map(|patch| crate::tools::patch_files(patch).join(", ")))
        .unwrap_or_default();
    let picked = url::Url::parse(&picked)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or(picked);
    let mut picked = after_cd(&picked)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for key in ["line", "character"] {
        if let Some(number) = value.get(key).and_then(serde_json::Value::as_u64) {
            picked.push_str(&format!(":{number}"));
        }
    }
    if let Some(op) = field("op").filter(|op| !QUIET_OPS.contains(op)) {
        if LISTING_OPS.contains(&op) {
            let dir = picked.trim_end_matches('/');
            picked = format!("{}/", if dir.is_empty() { "." } else { dir });
        } else {
            picked.push_str(&format!(" {op}"));
        }
    }
    if value.get("references").and_then(serde_json::Value::as_bool) == Some(true) {
        picked.push_str(" refs");
    }
    picked
}

fn after_cd(command: &str) -> &str {
    let mut rest = command;
    while let Some(target) = rest.strip_prefix("cd ") {
        let cut = [
            target.find("&&").map(|index| (index, 2)),
            target.find(';').map(|index| (index, 1)),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|(index, _)| *index);
        let Some((index, len)) = cut else { break };
        rest = target[index + len..].trim_start();
    }
    rest
}
