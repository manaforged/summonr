use std::ops::Range;

use sigil_tui::cell_width;
use unicode_linebreak::linebreaks;
use unicode_segmentation::UnicodeSegmentation;

pub(super) struct Input {
    pub rows: Vec<Range<usize>>,
}

impl Input {
    pub(super) fn new(text: &str, width: usize) -> Self {
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
        Self { rows }
    }
}
