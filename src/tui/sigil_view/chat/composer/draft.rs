use sigil::app::cell_width;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct Draft<'a> {
    before: &'a str,
    after: &'a str,
    left: bool,
    right: bool,
    cursor: bool,
}

impl<'a> Draft<'a> {
    pub(super) fn new(text: &'a str, caret: Option<usize>, width: usize) -> Self {
        let position = text.floor_char_boundary(caret.unwrap_or(text.len()).min(text.len()));
        let cursor = caret.is_some() && width > 0;
        let available = width.saturating_sub(usize::from(cursor));
        if cell_width(text) <= available {
            let (before, after) = text.split_at(position);
            return Self {
                before,
                after,
                left: false,
                right: false,
                cursor,
            };
        }
        if caret.is_none() {
            let right = cell_width(text) > available && available > 0;
            let end = prefix(text, available.saturating_sub(usize::from(right)));
            return Self {
                before: &text[..end],
                after: "",
                left: false,
                right,
                cursor,
            };
        }
        let (before, after) = text.split_at(position);
        let budget = if before.is_empty() {
            available
        } else if after.is_empty() {
            0
        } else {
            available / 2
        };
        let right = cell_width(after) > budget && budget > 0;
        let end = prefix(after, budget.saturating_sub(usize::from(right)));
        let after = &after[..end];
        let budget = available.saturating_sub(cell_width(after) + usize::from(right));
        let left = cell_width(before) > budget && budget > 0;
        let mut used = 0;
        let mut start = position;
        for (index, grapheme) in before.grapheme_indices(true).rev() {
            used += cell_width(grapheme);
            if used > budget.saturating_sub(usize::from(left)) {
                break;
            }
            start = index;
        }
        Self {
            before: &text[start..position],
            after,
            left,
            right,
            cursor,
        }
    }

    pub(super) fn text(&self, caret: char) -> String {
        let mut text = String::new();
        if self.left {
            text.push('…');
        }
        text.push_str(self.before);
        if self.cursor {
            text.push(caret);
        }
        text.push_str(self.after);
        if self.right {
            text.push('…');
        }
        text
    }
}

fn prefix(text: &str, width: usize) -> usize {
    let mut used = 0;
    text.grapheme_indices(true)
        .find_map(|(byte, grapheme)| {
            used += cell_width(grapheme);
            (used > width).then_some(byte)
        })
        .unwrap_or(text.len())
}
