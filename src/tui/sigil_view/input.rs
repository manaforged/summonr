use std::ops::Range;

use sigil_tui::cell_width;
use unicode_linebreak::linebreaks;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Caret {
    pub byte: usize,
    pub row: usize,
    pub column: usize,
    width: usize,
}

pub(super) struct Input<'a> {
    pub text: &'a str,
    pub rows: Vec<Range<usize>>,
    width: usize,
}

impl<'a> Input<'a> {
    pub(super) fn new(text: &'a str, width: usize) -> Self {
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut offset = 0;
        for line in text.split('\n') {
            if line.is_empty() {
                rows.push(offset..offset);
            } else {
                let breaks: Vec<_> = linebreaks(line).map(|(byte, _)| byte).collect();
                let mut start = 0;
                while start < line.len() {
                    let mut used = 0;
                    let mut limit = start;
                    for (byte, grapheme) in line[start..].grapheme_indices(true) {
                        let size = cell_width(grapheme);
                        if used + size > width && limit > start {
                            break;
                        }
                        used += size;
                        limit = start + byte + grapheme.len();
                    }
                    let end = if limit == line.len() {
                        limit
                    } else {
                        breaks
                            .partition_point(|byte| *byte <= limit)
                            .checked_sub(1)
                            .and_then(|index| breaks.get(index))
                            .copied()
                            .filter(|byte| *byte > start)
                            .unwrap_or(limit)
                    };
                    rows.push(offset + start..offset + end);
                    start = end;
                }
            }
            offset += line.len() + 1;
        }
        Self { text, rows, width }
    }

    pub(super) fn caret(&self, byte: usize, previous: Option<Caret>) -> Caret {
        let byte = self.text.floor_char_boundary(byte.min(self.text.len()));
        if let Some(previous) = previous.filter(|caret| {
            caret.byte == byte
                && caret.width == self.width
                && self
                    .rows
                    .get(caret.row)
                    .is_some_and(|row| row.start <= byte && byte <= row.end)
        }) {
            return previous;
        }
        let row = self
            .rows
            .partition_point(|row| row.start <= byte)
            .saturating_sub(1);
        Caret {
            byte,
            row,
            column: cell_width(&self.text[self.rows[row].start..byte]),
            width: self.width,
        }
    }

    pub(super) fn hit(&self, row: usize, column: usize) -> Option<Caret> {
        let range = self.rows.get(row)?;
        let mut used = 0;
        let byte = self.text[range.clone()]
            .grapheme_indices(true)
            .find_map(|(byte, grapheme)| {
                used += cell_width(grapheme);
                (column < used).then_some(range.start + byte)
            })
            .unwrap_or(range.end);
        Some(Caret {
            byte,
            row,
            column,
            width: self.width,
        })
    }

    pub(super) fn vertical(&self, caret: Caret, delta: isize) -> Option<Caret> {
        self.hit(caret.row.checked_add_signed(delta)?, caret.column)
    }

    pub(super) fn start(&self, caret: Caret, count: usize) -> usize {
        caret
            .row
            .saturating_sub(count.saturating_sub(1))
            .min(self.rows.len().saturating_sub(count))
    }
}

pub(super) fn width(cols: u16) -> usize {
    let cols = usize::from(cols);
    let margin = super::chat::margin(cols);
    cols.saturating_sub(margin * 2 + 7)
}
