use super::images::{decode_markdown_image, image_cells, markdown_image};
use super::syntax::{diagram, diagram_fence, inline, literal, numbered};
use super::table::{TableBuf, feed_table, flush_table};
use super::*;

pub(in crate::tui::sigil_view) fn markdown(text: &str, width: usize, accent: Rgba8) -> Rendered {
    let mut lines = Vec::new();
    let mut pictures = Vec::new();
    let mut fence = None;
    let mut table = None;
    let mut byte = 0;
    for raw in text.split_inclusive('\n') {
        let offset = byte;
        byte += raw.len();
        push_markdown_line(
            &mut lines,
            &mut pictures,
            text,
            offset,
            byte,
            &mut fence,
            &mut table,
            width,
            accent,
        );
    }
    flush_table(&mut lines, &mut table, text, width, accent);
    if lines.is_empty() {
        lines.push(Vec::new().into());
    }
    if let Some((true, start)) = fence {
        lines.extend(literal(&text[start..], start));
    }
    (lines, pictures)
}

#[expect(
    clippy::too_many_arguments,
    reason = "one call per markdown line, from the buffer it writes"
)]
fn push_markdown_line(
    lines: &mut Vec<Line>,
    pictures: &mut Vec<(usize, usize, usize, ImageSource)>,
    text: &str,
    offset: usize,
    end: usize,
    fence: &mut Option<(bool, usize)>,
    table: &mut Option<TableBuf>,
    width: usize,
    accent: Rgba8,
) {
    let raw = text[offset..end].trim_end_matches(['\r', '\n']);
    if let Some(info) = raw.trim_start().strip_prefix("```") {
        flush_table(lines, table, text, width, accent);
        match fence.take() {
            Some((true, start)) => {
                lines.extend(diagram(&text[start..offset], width, start..offset));
            }
            Some((false, _)) => {}
            None => *fence = Some((diagram_fence(info), end)),
        }
        return;
    }
    if let Some((mermaid, _)) = *fence {
        if !mermaid {
            lines.extend(literal(&text[offset..end], offset));
        }
        return;
    }
    if feed_table(lines, table, text, raw, offset, end, width, accent) {
        return;
    }
    let trimmed = raw.trim_start();
    if let Some(header) = trimmed
        .strip_prefix("### ")
        .or_else(|| trimmed.strip_prefix("## "))
        .or_else(|| trimmed.strip_prefix("# "))
    {
        lines.push(
            vec![
                Span::styled(header, accent)
                    .bold()
                    .source(offset + raw.len() - header.len()),
            ]
            .into(),
        );
    } else if let Some(item) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
    {
        let mut line = vec![Span::styled("• ", accent)];
        line.extend(inline(item, accent, offset + raw.len() - item.len()));
        lines.push(line.into());
    } else if let Some((number, item)) = numbered(trimmed) {
        let mut line = vec![Span::styled(format!("{number}. "), accent)];
        line.extend(inline(item, accent, offset + raw.len() - item.len()));
        lines.push(line.into());
    } else if let Some((alt, url)) = markdown_image(trimmed) {
        if let Some(source) = decode_markdown_image(url) {
            let (cols, rows) = image_cells(&source, width);
            lines.push(vec![Span::styled(alt, palette().dim)].into());
            let line = lines.len();
            lines.extend((0..rows).map(|_| vec![Span::text(" ")].into()));
            pictures.push((line, cols, rows, source));
        } else {
            lines.push(vec![Span::styled(format!("[{alt}]"), palette().dim)].into());
        }
    } else {
        lines.push(inline(raw, accent, offset).into());
    }
}

pub(in crate::tui::sigil_view) struct Stream {
    head: u64,
    cursor: usize,
    fence: Option<(bool, usize)>,
    table: Option<TableBuf>,
    lines: usize,
    usable: bool,
}

impl Stream {
    pub(in crate::tui::sigil_view) fn new(text: &str, width: usize, accent: Rgba8) -> Self {
        let commit = final_line_start(text);
        let mut fence = None;
        let mut table = None;
        let mut lines = 0;
        let mut byte = 0;
        while byte < commit {
            let end = line_end(text, byte, commit);
            let mut scratch = Vec::new();
            let mut pictures = Vec::new();
            push_markdown_line(
                &mut scratch,
                &mut pictures,
                text,
                byte,
                end,
                &mut fence,
                &mut table,
                width,
                accent,
            );
            lines += scratch.len();
            byte = end;
        }
        let usable = !matches!(fence, Some((true, _))) && table.as_ref().is_none_or(|buf| buf.sep);
        Self {
            head: head(text),
            cursor: commit,
            fence,
            table,
            lines,
            usable,
        }
    }

    pub(in crate::tui::sigil_view) fn matches(&self, text: &str) -> bool {
        self.usable
            && text.len() >= self.cursor
            && final_line_start(text) >= self.cursor
            && head(text) == self.head
    }

    pub(in crate::tui::sigil_view) const fn lines(&self) -> usize {
        self.lines
    }

    pub(in crate::tui::sigil_view) fn extend(
        &mut self,
        text: &str,
        width: usize,
        accent: Rgba8,
    ) -> (Vec<Line>, Vec<Line>) {
        let commit = final_line_start(text);
        let mut committed = Vec::new();
        let mut byte = self.cursor;
        while byte < commit {
            let end = line_end(text, byte, commit);
            let mut pictures = Vec::new();
            push_markdown_line(
                &mut committed,
                &mut pictures,
                text,
                byte,
                end,
                &mut self.fence,
                &mut self.table,
                width,
                accent,
            );
            byte = end;
        }
        self.cursor = commit;
        self.lines += committed.len();
        self.usable =
            !matches!(self.fence, Some((true, _))) && self.table.as_ref().is_none_or(|t| t.sep);
        let mut tail = Vec::new();
        if commit < text.len() {
            let mut fence = self.fence;
            let mut table = self.table.clone();
            let mut pictures = Vec::new();
            push_markdown_line(
                &mut tail,
                &mut pictures,
                text,
                commit,
                text.len(),
                &mut fence,
                &mut table,
                width,
                accent,
            );
        }
        (committed, tail)
    }
}

pub(super) fn line_end(text: &str, byte: usize, commit: usize) -> usize {
    text[byte..commit]
        .find('\n')
        .map_or(commit, |index| byte + index + 1)
}

pub(super) fn final_line_start(text: &str) -> usize {
    let end = text.len();
    if end > 0 && text.as_bytes()[end - 1] == b'\n' {
        text[..end - 1].rfind('\n').map_or(0, |index| index + 1)
    } else {
        text.rfind('\n').map_or(0, |index| index + 1)
    }
}

pub(super) fn head(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes().iter().take(64) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3_u64);
    }
    hash
}
