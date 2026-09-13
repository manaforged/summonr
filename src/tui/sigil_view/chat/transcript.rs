use sigil::prelude::ImageSource;

use sigil::prelude::Rgba8;
use std::ops::Range;

use super::{
    CODE_BG, CODE_COMMENT, CODE_KEYWORD, CODE_STRING, DIM, GREEN, Line, RED, Source, Span, TEXT,
    USER_BG, YELLOW, blend, fmt_tokens, pad, rgb,
};
use crate::tui::{Round, ToolState};

use super::super::model::{Entry, Model};

pub(super) fn entries(model: &Model, width: usize, accent: Rgba8) -> Vec<Vec<Line>> {
    let mut entries = Vec::new();
    let mut previous_tool = false;
    for entry in &model.entries {
        let mut lines = Vec::new();
        let tool = matches!(entry, Entry::Tool { .. });
        if !entries.is_empty() && !(tool && previous_tool) {
            lines.push(Vec::new());
        }
        previous_tool = tool;
        match entry {
            Entry::User(text) => lines.extend(user(text, width, accent)),
            Entry::Thinking(text) => {
                for (index, line) in text.lines().enumerate() {
                    lines.push(vec![
                        Span::styled(
                            format!("{}{line}", if index == 0 { "✻ " } else { "  " }),
                            DIM,
                        )
                        .italic(),
                    ]);
                }
            }
            Entry::Assistant { text, done } => {
                let mut rendered = markdown(text, width, accent);
                if !done && let Some(line) = rendered.last_mut() {
                    line.push(Span::styled("▌", DIM));
                }
                lines.extend(rendered);
            }
            Entry::Tool {
                name,
                arg,
                state,
                output,
            } => lines.extend(tool_lines(name, arg, state, model.prefs.tools.text(output))),
            Entry::Verify { command, passed } => lines.push(vec![
                Span::styled(
                    if *passed { "✓ " } else { "✗ " },
                    if *passed { GREEN } else { RED },
                ),
                Span::text("verify · "),
                Span::styled(command, DIM),
            ]),
            Entry::Mail { from, text } => {
                for (index, line) in text.lines().enumerate() {
                    lines.push(vec![
                        Span::styled(
                            if index == 0 {
                                format!("✉ {from} ")
                            } else {
                                "  ".to_string()
                            },
                            accent,
                        ),
                        Span::styled(line, DIM),
                    ]);
                }
            }
            Entry::Aborted(reason) => lines.push(vec![Span::styled(format!("⏹ {reason}"), YELLOW)]),
            Entry::System(text) => lines.extend(
                text.lines()
                    .map(|line| vec![Span::styled(line, DIM).italic()]),
            ),
            Entry::Error(text) => lines.push(vec![Span::styled(format!("summon: {text}"), RED)]),
            Entry::Chart(rounds) => lines.extend(chart(rounds, accent)),
            Entry::Image { source, label } => {
                let (_, rows) = image_cells(source, width);
                lines.push(vec![Span::styled(label, DIM)]);
                lines.extend((0..rows).map(|_| vec![Span::text(" ")]));
            }
        }
        entries.push(lines);
    }
    entries
}

fn user(text: &str, width: usize, accent: Rgba8) -> Vec<Line> {
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        rows.push(pad(
            vec![
                Span::surface(if index == 0 { "❯ " } else { "  " }, accent, USER_BG).bold(),
                Span::surface(line, TEXT, USER_BG),
            ],
            width,
            USER_BG,
        ));
    }
    if rows.is_empty() {
        rows.push(pad(
            vec![Span::surface("❯ ", accent, USER_BG).bold()],
            width,
            USER_BG,
        ));
    }
    rows
}

fn tool_lines(name: &str, arg: &str, state: &ToolState, tail: &str) -> Vec<Line> {
    let (icon, color) = match state {
        ToolState::Running => ("⠋", YELLOW),
        ToolState::Ok => ("✓", GREEN),
        ToolState::Failed => ("✗", RED),
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
    let mut line = vec![Span::styled(format!("{icon} "), color), Span::text(label)];
    if !arg.is_empty() {
        line.push(Span::styled(format!(" {arg}"), DIM));
    }
    let mut lines = vec![line];
    lines.extend(tail.lines().enumerate().map(|(index, line)| {
        vec![Span::styled(
            format!("{}{line}", if index == 0 { "  ⎿ " } else { "    " }),
            DIM,
        )]
    }));
    lines
}

fn markdown(text: &str, width: usize, accent: Rgba8) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut fence: Option<(bool, Vec<String>, usize)> = None;
    let mut byte = 0;
    for raw in text.split_inclusive('\n') {
        let offset = byte;
        byte += raw.len();
        let raw = raw.trim_end_matches(['\r', '\n']);
        if let Some(info) = raw.trim_start().strip_prefix("```") {
            match fence.take() {
                Some((true, source, start)) => {
                    lines.extend(diagram(&source.join("\n"), width, start..byte))
                }
                Some((false, _, _)) => {}
                None => fence = Some((info.trim().starts_with("mermaid"), Vec::new(), offset)),
            }
            continue;
        }
        if let Some((mermaid, source, _)) = fence.as_mut() {
            if *mermaid {
                source.push(raw.to_string());
            } else {
                let mut spans = code(raw);
                let mut source = offset;
                for span in spans.iter_mut().skip(1) {
                    span.source = Some(Source::Text(source..source + span.text.len()));
                    source += span.text.len();
                }
                lines.push(pad(spans, width, CODE_BG));
            }
            continue;
        }
        {
            let trimmed = raw.trim_start();
            if let Some(header) = trimmed
                .strip_prefix("### ")
                .or_else(|| trimmed.strip_prefix("## "))
                .or_else(|| trimmed.strip_prefix("# "))
            {
                lines.push(vec![
                    Span::styled(header, accent)
                        .bold()
                        .source(offset + raw.len() - header.len()),
                ]);
            } else if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                let mut line = vec![Span::styled("• ", accent)];
                line.extend(inline(item, accent, offset + raw.len() - item.len()));
                lines.push(line);
            } else if let Some((number, item)) = numbered(trimmed) {
                let mut line = vec![Span::styled(format!("{number}. "), accent)];
                line.extend(inline(item, accent, offset + raw.len() - item.len()));
                lines.push(line);
            } else {
                lines.push(inline(raw, accent, offset));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    if let Some((true, source, start)) = fence {
        lines.extend(source.iter().map(|line| {
            let mut spans = pad(code(line), width, CODE_BG);
            for span in &mut spans {
                span.source = Some(Source::Block(start..text.len()));
            }
            spans
        }));
    }
    lines
}

fn diagram(source: &str, width: usize, range: Range<usize>) -> Vec<Line> {
    sigil_tui::widgets::mermaid_draw(source)
        .into_iter()
        .map(|line| {
            let mut span = Span::surface(format!("  {line}"), TEXT, CODE_BG);
            span.source = Some(Source::Block(range.clone()));
            pad(vec![span], width, CODE_BG)
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

fn inline(text: &str, accent: Rgba8, offset: usize) -> Line {
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
                let mut span =
                    Span::styled(std::mem::take(&mut plain), if code { accent } else { TEXT });
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
        let mut span = Span::styled(plain, if code { accent } else { TEXT });
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

pub(crate) fn code(raw: &str) -> Line {
    let mut spans = vec![Span::surface("  ", TEXT, CODE_BG)];
    let mut text = raw;
    if let Some(index) = raw.find("//") {
        let (plain, comment) = raw.split_at(index);
        words(&mut spans, plain);
        spans.push(Span::surface(comment, CODE_COMMENT, CODE_BG));
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
            CODE_STRING,
            CODE_BG,
        ));
        text = &text[start + end + 2..];
    }
    words(&mut spans, text);
    spans
}

fn words(spans: &mut Line, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let word = rest
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        if word == 0 {
            let gap = rest
                .find(|c: char| c.is_alphanumeric() || c == '_')
                .unwrap_or(rest.len());
            spans.push(Span::surface(&rest[..gap], TEXT, CODE_BG));
            rest = &rest[gap..];
            continue;
        }
        let color = if KEYWORDS.contains(&&rest[..word]) {
            CODE_KEYWORD
        } else {
            TEXT
        };
        spans.push(Span::surface(&rest[..word], color, CODE_BG));
        rest = &rest[word..];
    }
}

fn chart(rounds: &[Round], accent: Rgba8) -> Vec<Line> {
    if rounds.is_empty() {
        return vec![vec![Span::styled(
            "no usage yet — ask something first",
            DIM,
        )]];
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
            Span::styled(format!("r{:<3} ", index + 1), DIM),
            Span::styled("█".repeat(hit), blend(fresh, rgb(10, 8, 20), 0.55)),
            Span::styled("█".repeat(bar.saturating_sub(hit).max(1)), fresh),
            Span::styled(format!(" {}", fmt_tokens(u64::from(round.total()))), DIM),
        ];
        if round.cached > 0 && round.prompt > 0 {
            line.push(Span::styled(
                format!(" · {}%", round.cached * 100 / round.prompt),
                DIM,
            ));
        }
        lines.push(line);
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
    lines.push(vec![Span::styled(summary, DIM)]);
    lines
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
