use base64::Engine;
use sigil::prelude::ImageSource;

use sigil::prelude::Rgba8;
use sigil_tui::cell_width;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;

use super::super::canvas::{Line, Source, Span, blend, palette, rgb};
use crate::prefs::{DiffLayout, Prefs, ToolOutput, Visibility};

pub(in crate::tui::sigil_view) use images::{fit_source, image_cells};
pub(in crate::tui::sigil_view) use stream::{Stream, markdown};
pub(in crate::tui::sigil_view) use syntax::code;
mod chart;
mod edits;
mod images;
mod stream;
mod syntax;
mod table;
use crate::tui::{Round, ToolState};

use super::super::model::Entry;

const CACHE_MAX: usize = 32;

thread_local! {
    static CACHE: RefCell<HashMap<(u64, u32, u32), ImageSource>> = RefCell::new(HashMap::new());
}

type Picture = (usize, usize, usize, ImageSource);
type Rendered = (Vec<Line>, Vec<Picture>);

pub(in crate::tui::sigil_view) fn entry_lines(
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
