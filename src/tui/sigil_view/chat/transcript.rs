use base64::Engine;
use sigil::prelude::ImageSource;

use sigil::prelude::Rgba8;
use sigil_tui::cell_width;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use super::super::canvas::palette;
use super::{Line, Source, Span, blend, rgb};
use crate::prefs::{DiffLayout, Prefs, ToolOutput, Visibility};

mod edits;
use crate::tui::{Round, ToolState};

use super::super::model::Entry;

const CACHE_MAX: usize = 32;

thread_local! {
    static CACHE: RefCell<HashMap<(u64, u32, u32), ImageSource>> = RefCell::new(HashMap::new());
}

type Picture = (usize, usize, usize, ImageSource);
type Rendered = (Vec<Line>, Vec<Picture>);

pub(super) fn entry_lines(
    entry: &Entry,
    width: usize,
    accent: Rgba8,
    prefs: &Prefs,
    separated: bool,
) -> Rendered {
    let reasoning = prefs.reasoning;
    let tools = prefs.tools;
    let receipts = prefs.receipts;
    let mut lines = Vec::new();
    let mut pictures = Vec::new();
    if separated {
        lines.push(Vec::new().into());
    }
    match entry {
        Entry::User(text) => lines.extend(user(text, accent)),
        Entry::Thinking(text) => match reasoning {
            Visibility::Off => {}
            Visibility::Collapsed => {
                lines.push(Vec::new().into());
                let count = text.lines().count();
                lines.push(
                    vec![
                        Span::styled(format!("⌄ thinking · {count} lines"), palette().thought)
                            .italic(),
                    ]
                    .into(),
                );
            }
            Visibility::Expanded => {
                lines.push(Vec::new().into());
                for (index, line) in text.lines().enumerate() {
                    lines.push(
                        vec![
                            Span::styled(
                                format!("{}{line}", if index == 0 { "✻ " } else { "  " }),
                                palette().thought,
                            )
                            .italic(),
                        ]
                        .into(),
                    );
                }
            }
        },
        Entry::Receipt(text) => match receipts {
            Visibility::Off => {}
            Visibility::Collapsed => {
                let count = text.lines().count();
                lines.push(
                    vec![Span::styled(
                        format!("⌄ receipt · {count} lines"),
                        palette().dim,
                    )]
                    .into(),
                );
            }
            Visibility::Expanded => lines.extend(
                text.lines()
                    .map(|line| vec![Span::styled(line, palette().dim).italic()].into()),
            ),
        },
        Entry::Assistant { text, done } => {
            let skip = lines.len();
            let (mut rendered, pics) = markdown(text, width, accent);
            if !done && let Some(line) = rendered.last_mut() {
                line.spans.push(Span::surface("▌", palette().dim, line.bg));
            }
            lines.extend(rendered);
            pictures.extend(
                pics.into_iter()
                    .map(|(line, cols, rows, source)| (line + skip, cols, rows, source)),
            );
        }
        Entry::Tool {
            name,
            arg,
            state,
            output,
        } => lines.extend(tool_lines(
            name,
            arg,
            state,
            output,
            tools,
            width,
            prefs.diff_layout,
        )),
        Entry::Verify { command, passed } => lines.push(
            vec![
                Span::styled(
                    if *passed { "✓ " } else { "✗ " },
                    if *passed {
                        palette().green
                    } else {
                        palette().error
                    },
                ),
                Span::text("verify · "),
                Span::styled(command, palette().dim),
            ]
            .into(),
        ),
        Entry::Mail { from, text } => {
            for (index, line) in text.lines().enumerate() {
                lines.push(
                    vec![
                        Span::styled(
                            if index == 0 {
                                format!("✉ {from} ")
                            } else {
                                "  ".to_string()
                            },
                            accent,
                        ),
                        Span::styled(line, palette().dim),
                    ]
                    .into(),
                );
            }
        }
        Entry::Aborted(reason) => {
            lines.push(vec![Span::styled(format!("⏹ {reason}"), palette().yellow)].into())
        }
        Entry::System(text) => lines.extend(
            text.lines()
                .map(|line| vec![Span::styled(line, palette().dim).italic()].into()),
        ),
        Entry::Error(text) => {
            lines.push(vec![Span::styled(format!("summon: {text}"), palette().error)].into())
        }
        Entry::Chart(rounds) => lines.extend(chart(rounds, accent)),
        Entry::Image { source, label } => {
            let (_, rows) = image_cells(source, width);
            lines.push(vec![Span::styled(label, palette().dim)].into());
            lines.extend((0..rows).map(|_| vec![Span::text(" ")].into()));
        }
    }
    (lines, pictures)
}

fn user(text: &str, accent: Rgba8) -> Vec<Line> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        rows.push(Line {
            spans: vec![
                Span::surface(
                    if index == 0 { "❯ " } else { "  " },
                    accent,
                    palette().user_bg,
                )
                .bold(),
                Span::surface(line, palette().text, palette().user_bg),
            ],
            bg: palette().user_bg,
        });
    }
    if rows.is_empty() {
        rows.push(Line {
            spans: vec![Span::surface("❯ ", accent, palette().user_bg).bold()],
            bg: palette().user_bg,
        });
    }
    rows
}

fn tool_lines(
    name: &str,
    arg: &str,
    state: &ToolState,
    output: &str,
    tools: ToolOutput,
    width: usize,
    layout: DiffLayout,
) -> Vec<Line> {
    if matches!(name, "edit" | "edit_file" | "write" | "write_file") {
        return edits::card(name, arg, state, output, tools, width, layout);
    }
    let tail = tools.text(output);
    let mut lines = tool_header(name, arg, state, width);
    let background = palette().tool_bg;
    lines.extend(tail.lines().map(|line| {
        let prefix = "  ";
        let (fg, bg) = output_color(line);
        Line {
            spans: vec![
                Span::surface(prefix, palette().dim, background),
                Span::surface(line, fg, bg),
            ],
            bg: background,
        }
    }));
    lines
}

fn tool_header(name: &str, arg: &str, state: &ToolState, width: usize) -> Vec<Line> {
    let (icon, color) = match state {
        ToolState::Running => ("⠋", palette().yellow),
        ToolState::Ok => ("✓", palette().green),
        ToolState::Failed => ("✗", palette().error),
    };
    let label = match name {
        "run_command" => "$",
        "read" | "read_file" => "read",
        "edit" | "edit_file" => "edit",
        "write" | "write_file" => "write",
        "list_dir" => "ls",
        "web" | "web_search" => "search",
        "web_fetch" => "fetch",
        "lsp" | "diagnostics" => "lsp",
        "agent" => "agent",
        other => other,
    };
    let background = palette().tool_bg;
    let icon_text = format!(" {icon} ");
    let room = width.saturating_sub(cell_width(&icon_text) + cell_width(label) + 1);
    let arg = if arg.is_empty() {
        String::new()
    } else {
        format!(" {}", super::ellipsize(arg, room.saturating_sub(1)))
    };
    vec![Line {
        spans: vec![
            Span::surface(icon_text, color, background),
            Span::surface(label, palette().text, background),
            Span::surface(arg, palette().gray, background),
        ],
        bg: background,
    }]
}

fn band(text: String, fg: Rgba8, bg: Rgba8, width: usize) -> Line {
    let pad = width.saturating_sub(cell_width(&text));
    Line {
        spans: vec![
            Span::surface(text, fg, bg),
            Span::surface(" ".repeat(pad), fg, bg),
        ],
        bg,
    }
}

fn output_color(line: &str) -> (Rgba8, Rgba8) {
    if line.starts_with("+++") || line.starts_with("---") {
        (palette().gray, palette().tool_bg)
    } else if line.starts_with('+') {
        (palette().green, palette().diff_add_bg)
    } else if line.starts_with('-') {
        (palette().red, palette().diff_del_bg)
    } else if line.starts_with("@@") {
        (palette().cyan, palette().tool_bg)
    } else {
        (palette().gray, palette().tool_bg)
    }
}

fn markdown(text: &str, width: usize, accent: Rgba8) -> Rendered {
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

#[derive(Clone)]
struct TableBuf {
    header: (usize, usize),
    rows: Vec<Vec<String>>,
    sep: bool,
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

fn markdown_image(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim().strip_prefix("![")?;
    let mid = rest.find("](")?;
    let alt = &rest[..mid];
    let url = rest[mid + 2..].strip_suffix(')')?;
    let url = url
        .trim()
        .trim_matches(|c| c == '<' || c == '>' || c == '"');
    Some((alt, url))
}

fn decode_markdown_image(url: &str) -> Option<ImageSource> {
    if let Some(data) = url.strip_prefix("data:") {
        let (meta, payload) = data.split_once(',')?;
        if !meta.contains("base64") || meta.contains("svg") {
            return None;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .ok()?;
        return super::super::model::preview(&bytes).ok();
    }
    if url.contains("://") {
        return None;
    }
    let path = std::path::Path::new(url);
    let bytes = std::fs::read(path).ok()?;
    super::super::model::preview(&bytes).ok()
}

#[expect(
    clippy::too_many_arguments,
    reason = "one call per table row, from the fence that produced it"
)]
fn feed_table(
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

fn flush_table(
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
    lines.push(inline(raw, accent, buf.header.0).into());
}

fn pipe_row(line: &str) -> Option<Vec<String>> {
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

fn is_table_sep(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    trimmed.trim_matches('|').split('|').all(|cell| {
        let cell = cell.trim();
        !cell.is_empty() && cell.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')
    })
}

fn paint_table(rows: &[Vec<String>], width: usize, accent: Rgba8) -> Vec<Line> {
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
                shown = super::ellipsize(cell, *w);
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

pub(super) struct Stream {
    head: u64,
    cursor: usize,
    fence: Option<(bool, usize)>,
    table: Option<TableBuf>,
    lines: usize,
    usable: bool,
}

impl Stream {
    pub(super) fn new(text: &str, width: usize, accent: Rgba8) -> Self {
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

    pub(super) fn matches(&self, text: &str) -> bool {
        self.usable
            && text.len() >= self.cursor
            && final_line_start(text) >= self.cursor
            && head(text) == self.head
    }

    pub(super) const fn lines(&self) -> usize {
        self.lines
    }

    pub(super) fn extend(
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

fn line_end(text: &str, byte: usize, commit: usize) -> usize {
    text[byte..commit]
        .find('\n')
        .map_or(commit, |index| byte + index + 1)
}

fn final_line_start(text: &str) -> usize {
    let end = text.len();
    if end > 0 && text.as_bytes()[end - 1] == b'\n' {
        text[..end - 1].rfind('\n').map_or(0, |index| index + 1)
    } else {
        text.rfind('\n').map_or(0, |index| index + 1)
    }
}

fn head(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in text.as_bytes().iter().take(64) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3_u64);
    }
    hash
}

fn literal(text: &str, offset: usize) -> impl Iterator<Item = Line> + '_ {
    text.split_inclusive('\n').scan(offset, |byte, raw| {
        let mut offset = *byte;
        *byte += raw.len();
        let mut spans = code(raw.trim_end_matches(['\r', '\n']));
        for span in spans.iter_mut().skip(1) {
            span.source = Some(Source::Text(offset..offset + span.text.len()));
            offset += span.text.len();
        }
        Some(Line {
            spans,
            bg: palette().code_bg,
        })
    })
}

fn diagram_fence(info: &str) -> bool {
    let info = info.trim();
    info.starts_with("mermaid") || info.eq_ignore_ascii_case("chart")
}

fn diagram(source: &str, width: usize, range: Range<usize>) -> Vec<Line> {
    let drawing = bars(source).unwrap_or_else(|| sigil_tui::widgets::mermaid_draw(source));
    if drawing.iter().any(|line| cell_width(line) + 2 > width) {
        let mut lines = vec![Line {
            spans: vec![Span::surface(
                "mermaid source · widen to view diagram",
                palette().dim,
                palette().code_bg,
            )],
            bg: palette().code_bg,
        }];
        lines.extend(literal(source, range.start));
        return lines;
    }
    drawing
        .into_iter()
        .map(|line| {
            let mut span = Span::surface(format!("  {line}"), palette().text, palette().code_bg);
            span.source = Some(Source::Block(range.clone()));
            Line {
                spans: vec![span],
                bg: palette().code_bg,
            }
        })
        .collect()
}

pub(crate) fn image_cells(source: &ImageSource, width: usize) -> (usize, usize) {
    let pixel_width = source.width() as usize;
    let pixel_height = source.height() as usize;
    if width == 0 || pixel_width == 0 || pixel_height == 0 {
        return (0, 0);
    }
    let aspect = super::super::canvas::cell_aspect();
    let cols = if aspect <= 1.0 {
        width.max(1)
    } else {
        width.min(pixel_width).max(1)
    };
    let rows = ((pixel_height as f32 * cols as f32) / (pixel_width as f32 * aspect))
        .ceil()
        .clamp(1.0, IMAGE_ROWS as f32) as usize;
    let cols = ((rows as f32 * aspect * pixel_width as f32) / pixel_height as f32)
        .round()
        .clamp(1.0, width as f32) as usize;
    (cols.max(1), rows.max(1))
}

pub(crate) fn paint_source(source: &ImageSource, cols: usize, rows: usize) -> ImageSource {
    if super::super::canvas::cell_aspect() <= 1.0 || sigil_tui::protocol().is_some() {
        source.clone()
    } else {
        display_source(source, cols, rows)
    }
}

pub(crate) fn display_source(source: &ImageSource, cols: usize, rows: usize) -> ImageSource {
    if cols == 0 || rows == 0 || sigil_tui::protocol().is_some() {
        return source.clone();
    }
    let width = u32::try_from(cols).unwrap_or(u32::MAX);
    let height = u32::try_from(rows.saturating_mul(2)).unwrap_or(u32::MAX);
    let key = (source.id(), width, height);
    if let Some(hit) = CACHE.with(|cache| cache.borrow().get(&key).cloned()) {
        return hit;
    }
    let resized = resized(source, width, height);
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= CACHE_MAX {
            cache.clear();
        }
        cache.insert(key, resized.clone());
    });
    resized
}

fn resized(source: &ImageSource, width: u32, height: u32) -> ImageSource {
    ::image::RgbaImage::from_raw(source.width(), source.height(), source.rgba().to_vec())
        .map(|buffer| {
            ::image::imageops::resize(
                &buffer,
                width,
                height,
                ::image::imageops::FilterType::Lanczos3,
            )
        })
        .and_then(|buffer| ImageSource::from_rgba8(width, height, buffer.into_raw()))
        .unwrap_or_else(|| source.clone())
}

pub(crate) fn numbered(line: &str) -> Option<(&str, &str)> {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 3 {
        return None;
    }
    let rest = line[digits..].strip_prefix(". ")?;
    Some((&line[..digits], rest))
}

fn inline(text: &str, accent: Rgba8, offset: usize) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut chars = text.char_indices().peekable();
    let mut start = 0;
    let mut bold = false;
    let mut code = false;
    while let Some((index, ch)) = chars.next() {
        let marker = (ch == '*' && chars.peek().is_some_and(|(_, ch)| *ch == '*')) || ch == '`';
        if marker {
            if ch == '*' {
                chars.next();
            }
            if !plain.is_empty() {
                let mut span = Span::styled(
                    std::mem::take(&mut plain),
                    if code { accent } else { palette().text },
                );
                span.bold = bold;
                span.source = Some(Source::Text(offset + start..offset + index));
                spans.push(span);
            }
            start = index + if ch == '*' { 2 } else { 1 };
            if ch == '*' {
                bold = !bold;
            } else {
                code = !code;
            }
        } else {
            plain.push(ch);
        }
    }
    if !plain.is_empty() || spans.is_empty() {
        let mut span = Span::styled(plain, if code { accent } else { palette().text });
        span.bold = bold;
        span.source = Some(Source::Text(offset + start..offset + text.len()));
        spans.push(span);
    }
    spans
}

const IMAGE_ROWS: usize = 48;

const BAR: usize = 24;

fn bars(source: &str) -> Option<Vec<String>> {
    xychart(source).or_else(|| series(source))
}

fn xychart(source: &str) -> Option<Vec<String>> {
    let mut title = None;
    let mut labels = Vec::new();
    let mut values = Vec::new();
    let mut saw = false;
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
    {
        let token = line.split_whitespace().next().unwrap_or("");
        if token == "xychart" || token.starts_with("xychart-beta") {
            saw = true;
            continue;
        }
        if !saw {
            return None;
        }
        if let Some(rest) = line.strip_prefix("title") {
            title = Some(rest.trim().trim_matches('"').to_string());
            continue;
        }
        if line.starts_with("x-axis") {
            labels = csv_items(line);
            continue;
        }
        if line.starts_with("y-axis") {
            continue;
        }
        if line.starts_with("bar") || line.starts_with("line") {
            values = csv_items(line)
                .into_iter()
                .filter_map(|item| item.parse::<f64>().ok())
                .collect();
        }
    }
    if !saw || values.is_empty() {
        return None;
    }
    if labels.len() != values.len() {
        labels = (1..=values.len()).map(|index| index.to_string()).collect();
    }
    Some(paint_bars(
        title,
        &labels
            .into_iter()
            .zip(values)
            .collect::<Vec<(String, f64)>>(),
    ))
}

fn series(source: &str) -> Option<Vec<String>> {
    let mut title = None;
    let mut rows = Vec::new();
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
    {
        let token = line.split_whitespace().next().unwrap_or("");
        if matches!(
            token,
            "graph"
                | "flowchart"
                | "stateDiagram"
                | "stateDiagram-v2"
                | "sequenceDiagram"
                | "pie"
                | "xychart"
                | "xychart-beta"
        ) {
            return None;
        }
        if rows.is_empty()
            && line
                .split_whitespace()
                .all(|word| word.parse::<f64>().is_err())
            && !line.contains(':')
            && !line.contains(',')
        {
            title = Some(line.trim_matches('"').to_string());
            continue;
        }
        rows.push(pair(line)?);
    }
    (!rows.is_empty()).then(|| paint_bars(title, &rows))
}

fn pair(line: &str) -> Option<(String, f64)> {
    if let Some((label, value)) = line.rsplit_once(':') {
        let value: f64 = value.trim().parse().ok()?;
        let label = label.trim().trim_matches('"').to_string();
        return (!label.is_empty() && value >= 0.0).then_some((label, value));
    }
    if let Some((label, value)) = line.rsplit_once(',') {
        let value: f64 = value.trim().parse().ok()?;
        let label = label.trim().trim_matches('"').to_string();
        return (!label.is_empty() && value >= 0.0).then_some((label, value));
    }
    let value = line.split_whitespace().next_back()?;
    let label = line[..line.len() - value.len()].trim().trim_matches('"');
    let value: f64 = value.parse().ok()?;
    (!label.is_empty() && value >= 0.0).then_some((label.to_string(), value))
}

fn csv_items(line: &str) -> Vec<String> {
    let Some(open) = line.find('[') else {
        return Vec::new();
    };
    let Some(close) = line.rfind(']') else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    line[open + 1..close]
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn paint_bars(title: Option<String>, rows: &[(String, f64)]) -> Vec<String> {
    let peak = rows.iter().map(|(_, value)| *value).fold(0.0, f64::max);
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    if let Some(title) = title {
        out.push(title);
    }
    for (label, value) in rows {
        let share = if peak > 0.0 { value / peak } else { 0.0 };
        let filled = usize::try_from((share.clamp(0.0, 1.0) * BAR as f64).round() as u64)
            .unwrap_or(BAR)
            .min(BAR);
        let pad = " ".repeat(width.saturating_sub(label.chars().count()));
        out.push(format!(
            "{label}{pad}  {}{}  {value}",
            "█".repeat(filled),
            "░".repeat(BAR.saturating_sub(filled)),
        ));
    }
    out
}

const KEYWORDS: [&str; 44] = [
    "as",
    "async",
    "await",
    "break",
    "class",
    "const",
    "continue",
    "def",
    "dyn",
    "else",
    "enum",
    "export",
    "false",
    "fn",
    "for",
    "from",
    "function",
    "if",
    "impl",
    "import",
    "in",
    "interface",
    "let",
    "loop",
    "match",
    "mod",
    "mut",
    "new",
    "null",
    "pub",
    "return",
    "self",
    "static",
    "struct",
    "trait",
    "true",
    "type",
    "unsafe",
    "use",
    "var",
    "where",
    "while",
    "None",
    "Some",
];

pub(crate) fn code(raw: &str) -> Vec<Span> {
    let mut spans = vec![Span::surface("  ", palette().text, palette().code_bg)];
    let mut text = raw;
    if let Some(index) = raw.find("//") {
        let (plain, comment) = raw.split_at(index);
        words(&mut spans, plain);
        spans.push(Span::surface(
            comment,
            palette().code_comment,
            palette().code_bg,
        ));
        return spans;
    }
    while let Some(start) = text.find(['\"', '\'']) {
        let quote = text.as_bytes()[start] as char;
        let after = &text[start + 1..];
        let Some(end) = after.find(quote) else {
            break;
        };
        words(&mut spans, &text[..start]);
        spans.push(Span::surface(
            &text[start..start + end + 2],
            palette().code_string,
            palette().code_bg,
        ));
        text = &text[start + end + 2..];
    }
    words(&mut spans, text);
    spans
}

fn words(spans: &mut Vec<Span>, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let word = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if word == 0 {
            let gap = rest
                .find(|c: char| c.is_alphanumeric() || c == '_')
                .unwrap_or(rest.len());
            spans.push(Span::surface(
                &rest[..gap],
                palette().text,
                palette().code_bg,
            ));
            rest = &rest[gap..];
            continue;
        }
        let color = if KEYWORDS.contains(&&rest[..word]) {
            palette().code_keyword
        } else {
            palette().text
        };
        spans.push(Span::surface(&rest[..word], color, palette().code_bg));
        rest = &rest[word..];
    }
}

pub(in crate::tui::sigil_view) fn chart(rounds: &[Round], accent: Rgba8) -> Vec<Line> {
    if rounds.is_empty() {
        return vec![
            vec![Span::styled(
                "no usage yet — ask something first",
                palette().dim,
            )]
            .into(),
        ];
    }
    let peak = rounds
        .iter()
        .map(|round| round.total())
        .max()
        .unwrap_or(0)
        .max(1);
    let mut lines = Vec::new();
    for (index, round) in rounds.iter().enumerate() {
        let bar =
            usize::try_from(round.total()).unwrap_or(0) * 24 / usize::try_from(peak).unwrap_or(1);
        let hit = if round.prompt > 0 {
            (usize::try_from(round.cached).unwrap_or(0) * bar
                / usize::try_from(round.prompt).unwrap_or(1))
            .min(bar)
        } else {
            0
        };
        let fresh = blend(
            accent,
            rgb(176, 200, 240),
            index as f32 / rounds.len().saturating_sub(1).max(1) as f32,
        );
        let mut line = vec![
            Span::styled(format!("r{:<3} ", index + 1), palette().dim),
            Span::styled("█".repeat(hit), blend(fresh, rgb(10, 8, 20), 0.55)),
            Span::styled("█".repeat(bar.saturating_sub(hit).max(1)), fresh),
            Span::styled(
                format!(" {}", crate::provider::compact(u64::from(round.total()))),
                palette().dim,
            ),
        ];
        if round.cached > 0 && round.prompt > 0 {
            line.push(Span::styled(
                format!(" · {}%", round.cached * 100 / round.prompt),
                palette().dim,
            ));
        }
        lines.push(line.into());
    }
    let total: u64 = rounds.iter().map(|round| u64::from(round.total())).sum();
    let cached: u64 = rounds.iter().map(|round| u64::from(round.cached)).sum();
    let prompted: u64 = rounds.iter().map(|round| u64::from(round.prompt)).sum();
    let mut summary = format!(
        "Σ {} across {} round{}",
        crate::provider::compact(total),
        rounds.len(),
        if rounds.len() == 1 { "" } else { "s" }
    );
    if cached > 0 && prompted > 0 {
        summary.push_str(&format!(" · {}% cached", cached * 100 / prompted));
    }
    if rounds.iter().any(|round| round.cost.is_some()) {
        summary.push_str(&format!(
            " · {}",
            crate::dollars(rounds.iter().filter_map(|round| round.cost).sum())
        ));
    }
    lines.push(vec![Span::styled(summary, palette().dim)].into());
    lines
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
