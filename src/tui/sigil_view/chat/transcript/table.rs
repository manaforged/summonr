use super::*;

#[derive(Clone)]
pub(super) struct TableBuf {
    pub(super) header: (usize, usize),
    pub(super) rows: Vec<Vec<String>>,
    pub(super) sep: bool,
}

#[expect(
    clippy::too_many_arguments,
    reason = "one call per table row, from the fence that produced it"
)]
pub(super) fn feed_table(
    lines: &mut Vec<Line>,
    table: &mut Option<TableBuf>,
    text: &str,
    raw: &str,
    offset: usize,
    end: usize,
    width: usize,
    accent: Rgba8,
) -> bool {
    if is_table_sep(raw) {
        if let Some(buf) = table.as_mut() {
            buf.sep = true;
            return true;
        }
        return false;
    }
    let Some(cells) = pipe_row(raw) else {
        flush_table(lines, table, text, width, accent);
        return false;
    };
    if let Some(buf) = table.as_mut() {
        if buf.sep {
            buf.rows.push(cells);
            return true;
        }
        flush_table(lines, table, text, width, accent);
    }
    *table = Some(TableBuf {
        header: (offset, end),
        rows: vec![cells],
        sep: false,
    });
    true
}

pub(super) fn flush_table(
    lines: &mut Vec<Line>,
    table: &mut Option<TableBuf>,
    text: &str,
    width: usize,
    accent: Rgba8,
) {
    let Some(buf) = table.take() else {
        return;
    };
    if buf.sep && !buf.rows.is_empty() {
        lines.extend(paint_table(&buf.rows, width, accent));
        return;
    }
    let raw = text[buf.header.0..buf.header.1].trim_end_matches(['\r', '\n']);
    lines.push(super::syntax::inline(raw, accent, buf.header.0).into());
}

pub(super) fn pipe_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let cells: Vec<String> = trimmed
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect();
    if cells.is_empty() {
        return None;
    }
    Some(cells)
}

pub(super) fn is_table_sep(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    trimmed.trim_matches('|').split('|').all(|cell| {
        let cell = cell.trim();
        !cell.is_empty() && cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
    })
}

pub(super) fn paint_table(rows: &[Vec<String>], width: usize, accent: Rgba8) -> Vec<Line> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 || width < 4 {
        return Vec::new();
    }
    let mut widths = vec![1; cols];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell_width(cell).max(1));
        }
    }
    let frame = cols * 3 + 1;
    let mut inner: usize = widths.iter().sum();
    let mut used = frame + inner;
    while used > width && inner > cols {
        if let Some(widest) = widths.iter_mut().max() {
            *widest -= 1;
        }
        inner = widths.iter().sum();
        used = frame + inner;
    }
    let rule = |left: char, mid: char, right: char| {
        let mut line = String::new();
        line.push(left);
        for (index, w) in widths.iter().enumerate() {
            line.push_str(&"─".repeat(*w + 2));
            line.push(if index + 1 == cols { right } else { mid });
        }
        line
    };
    let mut out = vec![band(
        rule('┌', '┬', '┐'),
        palette().gray,
        palette().code_bg,
        width,
    )];
    for (row_index, row) in rows.iter().enumerate() {
        let mut spans = vec![Span::surface("│", palette().gray, palette().code_bg)];
        for (index, w) in widths.iter().enumerate() {
            let cell = row.get(index).map(String::as_str).unwrap_or("");
            let mut shown = cell.to_string();
            if cell_width(&shown) > *w {
                shown = super::super::super::canvas::ellipsize(cell, *w);
            }
            let pad = w.saturating_sub(cell_width(&shown));
            let mut body = Span::surface(
                format!(" {shown}{} ", " ".repeat(pad)),
                if row_index == 0 {
                    accent
                } else {
                    palette().text
                },
                palette().code_bg,
            );
            body.bold = row_index == 0;
            spans.push(body);
            spans.push(Span::surface("│", palette().gray, palette().code_bg));
        }
        let pad = width.saturating_sub(cell_width(
            &spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
        ));
        if pad > 0 {
            spans.push(Span::surface(
                " ".repeat(pad),
                palette().gray,
                palette().code_bg,
            ));
        }
        out.push(Line {
            spans,
            bg: palette().code_bg,
        });
        if row_index == 0 && rows.len() > 1 {
            out.push(band(
                rule('├', '┼', '┤'),
                palette().gray,
                palette().code_bg,
                width,
            ));
        }
    }
    out.push(band(
        rule('└', '┴', '┘'),
        palette().gray,
        palette().code_bg,
        width,
    ));
    out
}
