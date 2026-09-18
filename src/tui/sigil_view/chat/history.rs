use sigil::prelude::{ImageSource, Rgba8};

use super::super::app::App;
use super::super::input::Input;
use super::super::model::{Entry, Model};
use super::chrome::line_width;
use super::{Line, Span, accent, canvas, margin, palette, transcript};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::tui::sigil_view) struct Anchor {
    pub(super) entry: usize,
    pub(super) line: usize,
    pub(super) byte: usize,
}

pub(in crate::tui::sigil_view) struct Row {
    pub(super) spans: Vec<Span>,
    pub(super) bg: Rgba8,
    pub(super) anchor: Anchor,
}

pub(in crate::tui::sigil_view) struct Placed {
    pub(super) line: usize,
    pub(super) cols: usize,
    pub(super) rows: usize,
    pub(super) source: ImageSource,
}

impl Placed {}

#[derive(Default)]
pub(in crate::tui::sigil_view) struct History {
    stamp: u64,
    width: usize,
    stable: usize,
    pub(super) lines: Vec<Row>,
    pub(super) images: Vec<Placed>,
    stream: Option<transcript::Stream>,
    #[cfg(test)]
    pub(super) rendered: usize,
}

impl History {
    pub(in crate::tui::sigil_view) fn diff_hit(
        &self,
        line: usize,
        col: usize,
    ) -> Option<crate::prefs::DiffLayout> {
        let mut x = 0;
        for span in &self.lines.get(line)?.spans {
            let end = x + sigil_tui::cell_width(&span.text);
            if (x..end).contains(&col) {
                return span.diff_layout;
            }
            x = end;
        }
        None
    }
    pub(in crate::tui::sigil_view) fn anchor(&self, position: f32) -> Option<Anchor> {
        self.lines
            .get((position / canvas::cell().h) as usize)
            .map(|row| row.anchor)
    }

    pub(in crate::tui::sigil_view) fn locate(&self, anchor: Anchor) -> Option<f32> {
        let row = self
            .lines
            .partition_point(|row| row.anchor <= anchor)
            .saturating_sub(1);
        self.lines
            .get(row)
            .filter(|row| row.anchor.entry == anchor.entry)?;
        Some(row as f32 * canvas::cell().h)
    }

    pub(in crate::tui::sigil_view) fn extent(
        &mut self,
        model: &Model,
        cols: u16,
        height: u16,
        stamp: u64,
    ) -> f32 {
        let width = usize::from(cols);
        let inset = margin(width);
        self.lines(model, width.saturating_sub(inset * 2), stamp)
            .len()
            .saturating_sub(usize::from(height)) as f32
            * canvas::cell().h
    }

    pub(in crate::tui::sigil_view) fn lines(
        &mut self,
        model: &Model,
        width: usize,
        stamp: u64,
    ) -> &[Row] {
        let len = model.entries.len();
        if let Some(Entry::Assistant { text, done: false }) =
            model.entries.get(len.saturating_sub(1))
            && self.width == width
            && self.stable == len
            && self
                .stream
                .as_ref()
                .is_some_and(|stream| stream.matches(text))
        {
            let index = len - 1;
            let accent = accent(&model.prefs.theme);
            let first = self.stream.as_ref().map_or(0, transcript::Stream::lines);
            let (committed, mut tail) = self
                .stream
                .as_mut()
                .expect("stream checked above")
                .extend(text, width, accent);
            let keep = self.lines.partition_point(|row| {
                row.anchor.entry < index || (row.anchor.entry == index && row.anchor.line < first)
            });
            self.lines.truncate(keep);
            self.images.retain(|placed| placed.line < keep);
            if committed.is_empty() && tail.is_empty() && self.lines.is_empty() {
                tail.push(Vec::new().into());
            }
            if let Some(last) = tail.last_mut() {
                last.spans.push(Span::surface("▌", palette().dim, last.bg));
            }
            let tail_first = first + committed.len();
            self.lines.extend(wrap(committed, width, index, first));
            self.lines.extend(wrap(tail, width, index, tail_first));
            self.stamp = stamp;
            self.width = width;
            self.stable = len;
            return &self.lines;
        }
        let start = if self.width != width || len < self.stable {
            0
        } else if len > self.stable || self.stamp != stamp {
            self.stable.saturating_sub(1)
        } else {
            self.stable
        };
        if start == 0 {
            self.lines.clear();
            self.images.clear();
        } else {
            let split = self.lines.partition_point(|row| row.anchor.entry < start);
            self.lines.truncate(split);
            self.images.retain(|placed| placed.line < split);
        }
        let accent = accent(&model.prefs.theme);
        let mut previous_tool =
            start > 0 && matches!(&model.entries[start - 1], Entry::Tool { .. });
        for index in start..len {
            let entry = &model.entries[index];
            let separated = index > 0 && !(previous_tool && matches!(entry, Entry::Tool { .. }));
            let (rendered, pictures) =
                transcript::entry_lines(entry, width, accent, &model.prefs, separated);
            let before = self.lines.len();
            self.lines.extend(wrap(rendered, width, index, 0));
            for (row, cols, rows, source) in pictures {
                if rows > 0 {
                    self.images.push(Placed {
                        line: before + row,
                        cols,
                        rows,
                        source,
                    });
                }
            }
            if let Entry::Image { source, .. } = entry {
                let (cols, _) = transcript::image_cells(source, width);
                let line = before + usize::from(separated) + 1;
                let rows = self.lines.len().saturating_sub(line);
                if rows > 0 {
                    self.images.push(Placed {
                        line,
                        cols,
                        rows,
                        source: source.clone(),
                    });
                }
            }
            previous_tool = matches!(entry, Entry::Tool { .. });
        }
        self.stream = match model.entries.last() {
            Some(Entry::Assistant { text, done: false }) => {
                Some(transcript::Stream::new(text, width, accent))
            }
            _ => None,
        };
        #[cfg(test)]
        {
            self.rendered += len - start;
        }
        self.stamp = stamp;
        self.width = width;
        self.stable = len;
        &self.lines
    }

    pub(in crate::tui::sigil_view) fn position(
        &mut self,
        app: &App,
        entry: usize,
    ) -> Result<Option<f32>, String> {
        let width = usize::from(app.chat_cols());
        let left = margin(width);
        let width = width.saturating_sub(left * 2).max(1);
        let model = app.model.borrow();
        self.lines(&model, width, model.entries.stamp);
        let start = self.lines.partition_point(|row| row.anchor.entry < entry);
        let end = self.lines.partition_point(|row| row.anchor.entry <= entry);
        if start == end {
            return Ok(None);
        }
        let lines = &self.lines[start..end];
        let pattern = app.search.pattern()?;
        if let Some(Entry::Assistant { text, .. }) = model.entries.get(entry)
            && let Some(found) = pattern.find(text)
        {
            let row = lines
                .iter()
                .position(|line| {
                    line.spans.iter().any(|span| {
                        span.source.as_ref().is_some_and(|source| {
                            source.range().end > found.start() && source.range().start < found.end()
                        })
                    })
                })
                .or_else(|| {
                    lines.iter().position(|line| {
                        line.spans.iter().any(|span| {
                            span.source
                                .as_ref()
                                .is_some_and(|source| source.range().start >= found.start())
                        })
                    })
                })
                .unwrap_or_else(|| lines.len().saturating_sub(1));
            return Ok(Some((start + row) as f32 * canvas::cell().h));
        }
        let mut text = String::new();
        let mut ends = Vec::with_capacity(lines.len());
        let mut previous = None;
        for line in lines {
            if previous
                .is_some_and(|(number, end)| line.anchor.line != number || line.anchor.byte > end)
            {
                text.push('\n');
            }
            let start = text.len();
            for span in &line.spans {
                text.push_str(&span.text);
            }
            previous = Some((line.anchor.line, line.anchor.byte + text.len() - start));
            ends.push(text.len());
        }
        let row = pattern
            .find(&text)
            .map_or(0, |found| ends.partition_point(|end| *end <= found.start()));
        Ok(Some((start + row) as f32 * canvas::cell().h))
    }
}

fn wrap(lines: Vec<Line>, width: usize, entry: usize, first_line: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, line) in lines.into_iter().enumerate() {
        let anchor = Anchor {
            entry,
            line: first_line + index,
            byte: 0,
        };
        if line_width(&line.spans) <= width
            && line.spans.iter().all(|span| !span.text.contains('\n'))
        {
            rows.push(Row {
                spans: line.spans,
                bg: line.bg,
                anchor,
            });
            continue;
        }
        let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
        let mut spans = line.spans.into_iter().peekable();
        let mut offset = 0;
        for range in Input::new(&text, width).rows {
            let mut row = Vec::new();
            while let Some(span) = spans.peek() {
                let length = span.text.len();
                if offset + length <= range.start {
                    offset += length;
                    drop(spans.next());
                    continue;
                }
                if offset >= range.end {
                    break;
                }
                let start = range.start.saturating_sub(offset);
                let end = range.end.saturating_sub(offset).min(length);
                if start < end {
                    row.push(span.slice(start..end));
                }
                if end < length {
                    break;
                }
                offset += length;
                drop(spans.next());
            }
            rows.push(Row {
                spans: row,
                bg: line.bg,
                anchor: Anchor {
                    byte: range.start,
                    ..anchor
                },
            });
        }
    }
    rows
}
