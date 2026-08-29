use sigil::prelude::Rgba8;

use super::{
    CODE_BG, CODE_COMMENT, CODE_STRING, DIM, GREEN, Line, RED, Span, TEXT, USER_BG, YELLOW, blend,
    fmt_tokens, pad, rgb,
};
use crate::tui::{Round, ToolState};

use super::super::model::{Entry, Model};

pub(super) fn entries(model: &Model, width: usize, accent: Rgba8) -> Vec<Line> {
    let mut lines = Vec::new();
    let mut previous_tool = false;
    for entry in &model.entries {
        let tool = matches!(entry, Entry::Tool { .. });
        if !lines.is_empty() && !(tool && previous_tool) {
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
                tail,
            } => lines.extend(tool_lines(name, arg, state, tail)),
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
            Entry::Image { bytes, label, .. } => lines.extend(
                super::image::render_cached(bytes, width)
                    .map(|rows| {
                        let mut block = vec![vec![Span::styled(label, DIM)]];
                        block.extend(rows);
                        block
                    })
                    .unwrap_or_else(|_| {
                        vec![vec![Span::styled(
                            format!("summon: {label} — could not render"),
                            RED,
                        )]]
                    }),
            ),
        }
    }
    lines
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
    let mut fence = false;
    for raw in text.lines() {
        if raw.trim_start().starts_with("```") {
            fence = !fence;
        } else if fence {
            lines.push(pad(code(raw), width, CODE_BG));
        } else {
            let trimmed = raw.trim_start();
            if let Some(header) = trimmed
                .strip_prefix("### ")
                .or_else(|| trimmed.strip_prefix("## "))
                .or_else(|| trimmed.strip_prefix("# "))
            {
                lines.push(vec![Span::styled(header, accent).bold()]);
            } else if let Some(item) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                let mut line = vec![Span::styled("• ", accent)];
                line.extend(inline(item, accent));
                lines.push(line);
            } else {
                lines.push(inline(raw, accent));
            }
        }
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

fn inline(text: &str, accent: Rgba8) -> Line {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    let mut bold = false;
    let mut code = false;
    while let Some(ch) = chars.next() {
        let marker = (ch == '*' && chars.peek() == Some(&'*')) || ch == '`';
        if marker {
            if ch == '*' {
                chars.next();
            }
            if !plain.is_empty() {
                let mut span =
                    Span::styled(std::mem::take(&mut plain), if code { accent } else { TEXT });
                span.bold = bold;
                spans.push(span);
            }
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
        spans.push(span);
    }
    spans
}

fn code(raw: &str) -> Line {
    let mut spans = vec![Span::surface("  ", TEXT, CODE_BG)];
    let mut text = raw;
    if let Some(index) = raw.find("//") {
        let (plain, comment) = raw.split_at(index);
        spans.push(Span::surface(plain, TEXT, CODE_BG));
        spans.push(Span::surface(comment, CODE_COMMENT, CODE_BG));
        return spans;
    }
    while let Some(start) = text.find(['\"', '\'']) {
        let quote = text.as_bytes()[start] as char;
        let after = &text[start + 1..];
        let Some(end) = after.find(quote) else {
            break;
        };
        spans.push(Span::surface(&text[..start], TEXT, CODE_BG));
        spans.push(Span::surface(
            &text[start..start + end + 2],
            CODE_STRING,
            CODE_BG,
        ));
        text = &text[start + end + 2..];
    }
    spans.push(Span::surface(text, TEXT, CODE_BG));
    spans
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
    lines.push(vec![Span::styled(summary, DIM)]);
    lines
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
