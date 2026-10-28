use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) enum Span {
    Cells(usize),
    Bytes(usize),
}

impl From<usize> for Span {
    fn from(cells: usize) -> Self {
        Self::Cells(cells)
    }
}

pub(crate) fn ellipsize(text: &str, span: impl Into<Span>) -> String {
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
