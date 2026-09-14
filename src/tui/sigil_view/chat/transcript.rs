use sigil::prelude::ImageSource;

use sigil::prelude::Rgba8;
use sigil_tui::cell_width;
use std::ops::Range;

use super::super::canvas::palette;
use super::{Line, Source, Span, blend, fmt_tokens, rgb};
use crate::prefs::{Reasoning, Receipts, ToolOutput};
use crate::tui::{Round, ToolState};

use super::super::model::Entry;

pub(super) fn entry_lines(
    entry: &Entry,
    width: usize,
    accent: Rgba8,
    reasoning: Reasoning,
    tools: ToolOutput,
    receipts: Receipts,
    separated: bool,
) -> Vec<Line> {
    let mut lines = Vec::new();
    if separated {
        lines.push(Vec::new().into());
    }
    match entry {
        Entry::User(text) => lines.extend(user(text, accent)),
        Entry::Thinking(text) => match reasoning {
            Reasoning::Off => {}
            Reasoning::Collapsed => {
                let count = text.lines().count();
                lines.push(
                    vec![Span::styled(
                        format!("⌄ thinking · {count} lines"),
                        palette().thought,
                    )]
                    .into(),
                );
            }
            Reasoning::Expanded => {
                for (index, line) in text.lines().enumerate() {
                    lines.push(Line {
                        spans: vec![
                            Span::surface(
                                format!("{}{line}", if index == 0 { "✻ " } else { "  " }),
                                palette().thought,
                                palette().thought_bg,
                            )
                            .italic(),
                        ],
                        bg: palette().thought_bg,
                    });
                }
            }
        },
        Entry::Receipt(text) => match receipts {
            Receipts::Off => {}
            Receipts::Collapsed => {
                let count = text.lines().count();
                lines.push(
                    vec![Span::styled(
                        format!("⌄ receipt · {count} lines"),
                        palette().dim,
                    )]
                    .into(),
                );
            }
            Receipts::Expanded => lines.extend(
                text.lines()
                    .map(|line| vec![Span::styled(line, palette().dim).italic()].into()),
            ),
        },
        Entry::Assistant { text, done } => {
            let mut rendered = markdown(text, width, accent);
            if !done && let Some(line) = rendered.last_mut() {
                line.spans.push(Span::surface("▌", palette().dim, line.bg));
            }
            lines.extend(rendered);
        }
        Entry::Tool {
            name,
            arg,
            state,
            output,
        } => lines.extend(tool_lines(name, arg, state, tools.text(output))),
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
    lines
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

fn tool_lines(name: &str, arg: &str, state: &ToolState, tail: &str) -> Vec<Line> {
    let (icon, color) = match state {
        ToolState::Running => ("⠋", palette().yellow),
        ToolState::Ok => ("✓", palette().green),
        ToolState::Failed => ("✗", palette().error),
    };
    let label = match name {
        "run_command" => "$",
        "read_file" => "read",
        "edit_file" => "edit",
        "write_file" => "write",
        "list_dir" => "ls",
        "web_search" => "search",
        "web_fetch" => "fetch",
        other => other,
    };
    let background = palette().tool_bg;
    let mut header = vec![
        Span::surface(format!(" {icon} "), color, background),
        Span::surface(label, palette().text, background),
    ];
    if !arg.is_empty() {
        header.push(Span::surface(format!(" {arg}"), palette().dim, background));
    }
    let mut lines = vec![Line {
        spans: header,
        bg: background,
    }];
    lines.extend(tail.lines().enumerate().map(|(index, line)| {
        let prefix = if index == 0 { "  ⎿ " } else { "    " };
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

fn output_color(line: &str) -> (Rgba8, Rgba8) {
    if line.starts_with("+++") || line.starts_with("---") {
        (palette().dim, palette().tool_bg)
    } else if line.starts_with('+') {
        (palette().green, palette().diff_add_bg)
    } else if line.starts_with('-') {
        (palette().red, palette().diff_del_bg)
    } else if line.starts_with("@@") {
        (palette().cyan, palette().tool_bg)
    } else {
        (palette().dim, palette().tool_bg)
    }
}

fn markdown(text: &str, width: usize, accent: Rgba8) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut fence = None;
    let mut byte = 0;
    for raw in text.split_inclusive('\n') {
        let offset = byte;
        byte += raw.len();
        push_markdown_line(&mut lines, text, offset, byte, &mut fence, width, accent);
    }
    if lines.is_empty() {
        lines.push(Vec::new().into());
    }
    if let Some((true, start)) = fence {
        lines.extend(literal(&text[start..], start));
    }
    lines
}

fn push_markdown_line(
    lines: &mut Vec<Line>,
    text: &str,
    offset: usize,
    end: usize,
    fence: &mut Option<(bool, usize)>,
    width: usize,
    accent: Rgba8,
) {
    let raw = text[offset..end].trim_end_matches(['\r', '\n']);
    if let Some(info) = raw.trim_start().strip_prefix("```") {
        match fence.take() {
            Some((true, start)) => {
                lines.extend(diagram(&text[start..offset], width, start..offset));
            }
            Some((false, _)) => {}
            None => *fence = Some((info.trim().starts_with("mermaid"), end)),
        }
        return;
    }
    if let Some((mermaid, _)) = *fence {
        if !mermaid {
            lines.extend(literal(&text[offset..end], offset));
        }
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
    } else {
        lines.push(inline(raw, accent, offset).into());
    }
}

pub(super) struct Stream {
    head: u64,
    cursor: usize,
    fence: Option<(bool, usize)>,
    lines: usize,
    usable: bool,
}

impl Stream {
    pub(super) fn new(text: &str, width: usize, accent: Rgba8) -> Self {
        let commit = final_line_start(text);
        let mut fence = None;
        let mut lines = 0;
        let mut byte = 0;
        while byte < commit {
            let end = line_end(text, byte, commit);
            let mut scratch = Vec::new();
            push_markdown_line(&mut scratch, text, byte, end, &mut fence, width, accent);
            lines += scratch.len();
            byte = end;
        }
        Self {
            head: head(text),
            cursor: commit,
            fence,
            lines,
            usable: !matches!(fence, Some((true, _))),
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
            push_markdown_line(
                &mut committed,
                text,
                byte,
                end,
                &mut self.fence,
                width,
                accent,
            );
            byte = end;
        }
        self.cursor = commit;
        self.lines += committed.len();
        self.usable = !matches!(self.fence, Some((true, _)));
        let mut tail = Vec::new();
        if commit < text.len() {
            let mut fence = self.fence;
            push_markdown_line(
                &mut tail,
                text,
                commit,
                text.len(),
                &mut fence,
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

fn diagram(source: &str, width: usize, range: Range<usize>) -> Vec<Line> {
    let drawing = sigil_tui::widgets::mermaid_draw(source);
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
    let cols = width.min(pixel_width).max(1);
    let rows = (pixel_height * cols / pixel_width)
        .div_ceil(2)
        .clamp(1, IMAGE_ROWS);
    let cols = cols.min((rows * 2 * pixel_width / pixel_height).max(1));
    (cols, rows)
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

const IMAGE_ROWS: usize = 24;

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

fn chart(rounds: &[Round], accent: Rgba8) -> Vec<Line> {
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
                format!(" {}", fmt_tokens(u64::from(round.total()))),
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
        fmt_tokens(total),
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
