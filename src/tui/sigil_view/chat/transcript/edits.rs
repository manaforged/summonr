use std::ops::Range;

use super::super::super::{
    input::Input,
    patch::{self, Kind, Pair},
};
use super::super::canvas::ellipsize;
use super::super::cell_width;
use super::{
    DiffLayout, Line, Span, ToolOutput, ToolState, band, blend, code, palette, tool_header,
};

pub(super) fn card(
    name: &str,
    path: &str,
    state: &ToolState,
    output: &str,
    tools: ToolOutput,
    width: usize,
    layout: DiffLayout,
) -> Vec<Line> {
    let patch = output
        .lines()
        .skip_while(|line| !line.starts_with("@@ "))
        .collect::<Vec<_>>()
        .join("\n");
    if patch.is_empty() {
        let mut lines = tool_header(name, path, state, width);
        lines.extend(tools.text(output).lines().map(|line| {
            band(
                format!("  {line}"),
                palette().gray,
                palette().tool_bg,
                width,
            )
        }));
        return lines;
    }
    if width < 24 {
        return patch
            .lines()
            .map(|line| {
                band(
                    ellipsize(line, width),
                    palette().text,
                    palette().tool_bg,
                    width,
                )
            })
            .collect();
    }
    let pairs = patch::pairs(&patch);
    let added = pairs
        .iter()
        .filter(|pair| pair.kind == Kind::Change && pair.right.is_some())
        .count();
    let removed = pairs
        .iter()
        .filter(|pair| pair.kind == Kind::Change && pair.left.is_some())
        .count();
    let split = layout == DiffLayout::Split && width >= patch::SPLIT;
    let shown_layout = if split {
        DiffLayout::Split
    } else {
        DiffLayout::Unified
    };
    let bg = palette().tool_bg;
    let mut header = vec![Span::surface("╭ ", palette().dim, bg)];
    let counts = format!("  +{added} −{removed}  ");
    let controls = width >= 40;
    let room = width.saturating_sub(cell_width(&counts) + if controls { 20 } else { 4 });
    let path = ellipsize(path, room);
    header.push(
        Span::surface(
            format!(
                "{path}{}",
                " ".repeat(room.saturating_sub(cell_width(&path)))
            ),
            palette().text,
            bg,
        )
        .bold(),
    );
    header.push(Span::surface(format!("  +{added}"), palette().green, bg));
    header.push(Span::surface(format!(" −{removed}  "), palette().red, bg));
    for choice in [DiffLayout::Split, DiffLayout::Unified]
        .into_iter()
        .filter(|_| controls)
    {
        let mut span = Span::surface(
            format!(" {} ", choice.name()),
            if choice == shown_layout {
                palette().text
            } else {
                palette().dim
            },
            if choice == shown_layout {
                palette().code_bg
            } else {
                bg
            },
        );
        span.diff_layout = Some(choice);
        header.push(span);
    }
    header.push(Span::surface(" ╮", palette().dim, bg));
    let mut lines = vec![Line { spans: header, bg }];
    let inner = width - 2;
    if split {
        let half = (inner - 1) / 2;
        let mut labels = padded(" Before", half);
        labels.push(Span::surface("│", palette().dim, bg));
        labels.extend(padded(" After", inner - half - 1));
        lines.push(framed(labels));
    }
    let limit = if tools == ToolOutput::Compact {
        32
    } else {
        usize::MAX
    };
    let mut old = 0;
    let mut new = 0;
    let mut index = 0;
    while let Some(pair) = pairs.get(index) {
        if lines.len() >= limit {
            lines.push(framed(padded(
                " … expand tool output to see the remaining changes",
                inner,
            )));
            break;
        }
        if !split && pair.kind == Kind::Change {
            let count = pairs[index..]
                .iter()
                .take_while(|pair| pair.kind == Kind::Change)
                .count();
            let changed = &pairs[index..index + count];
            lines.extend(unified_rows(changed, old, new, inner));
            old += changed.iter().filter(|pair| pair.left.is_some()).count();
            new += changed.iter().filter(|pair| pair.right.is_some()).count();
            index += count;
            continue;
        }
        if pair.kind == Kind::Hunk {
            if let Some((from, to)) = pair.left.as_deref().and_then(starts) {
                old = from;
                new = to;
            }
            let text = ellipsize(pair.left.as_deref().unwrap_or(""), inner.saturating_sub(2));
            lines.push(framed(vec![Span::surface(
                format!(
                    " {text}{}",
                    " ".repeat(inner.saturating_sub(cell_width(&text) + 1))
                ),
                palette().accent,
                bg,
            )]));
        } else if matches!(pair.kind, Kind::Context | Kind::Change) {
            lines.extend(if split {
                split_row(pair, old, new, inner)
            } else {
                source(pair.right.as_deref(), new, ' ', inner, None)
                    .into_iter()
                    .map(framed)
                    .collect()
            });
            old += usize::from(pair.left.is_some());
            new += usize::from(pair.right.is_some());
        } else if let Some(text) = pair.left.as_deref().filter(|line| !line.is_empty()) {
            lines.push(framed(padded(text, inner)));
        }
        index += 1;
    }
    let label = match state {
        ToolState::Running => " editing ",
        ToolState::Ok => " ✓ Applied ",
        ToolState::Failed => " ✗ Failed ",
    };
    lines.push(band(
        format!(
            "╰{label}{}╯",
            "─".repeat(width.saturating_sub(cell_width(label) + 2))
        ),
        palette().dim,
        bg,
        width,
    ));
    lines
}

fn padded(text: &str, width: usize) -> Vec<Span> {
    let text = ellipsize(text, width);
    vec![Span::surface(
        format!(
            "{text}{}",
            " ".repeat(width.saturating_sub(cell_width(&text)))
        ),
        palette().dim,
        palette().tool_bg,
    )]
}

fn framed(mut spans: Vec<Span>) -> Line {
    spans.insert(0, Span::surface("│", palette().dim, palette().tool_bg));
    spans.push(Span::surface("│", palette().dim, palette().tool_bg));
    Line {
        spans,
        bg: palette().tool_bg,
    }
}

fn starts(line: &str) -> Option<(usize, usize)> {
    let mut parts = line.strip_prefix("@@ ")?.split_whitespace();
    let old = parts
        .next()?
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let new = parts
        .next()?
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some((old, new))
}

fn changed(left: &str, right: &str) -> (Range<usize>, Range<usize>) {
    let prefix = left
        .chars()
        .zip(right.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    let suffix = left[prefix..]
        .chars()
        .rev()
        .zip(right[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum::<usize>();
    (prefix..left.len() - suffix, prefix..right.len() - suffix)
}

fn split_row(pair: &Pair, old: usize, new: usize, width: usize) -> Vec<Line> {
    let half = (width - 1) / 2;
    let left = pair.left.as_deref().map(|text| text.replace('\t', "    "));
    let right = pair.right.as_deref().map(|text| text.replace('\t', "    "));
    let ranges = (pair.kind == Kind::Change).then(|| {
        changed(
            left.as_deref().unwrap_or(""),
            right.as_deref().unwrap_or(""),
        )
    });
    let left = source(
        left.as_deref(),
        old,
        if ranges.is_some() { '-' } else { ' ' },
        half,
        ranges.as_ref().map(|r| r.0.clone()),
    );
    let right = source(
        right.as_deref(),
        new,
        if ranges.is_some() { '+' } else { ' ' },
        width - half - 1,
        ranges.map(|r| r.1),
    );
    (0..left.len().max(right.len()))
        .map(|index| {
            let mut row = left.get(index).cloned().unwrap_or_else(|| padded("", half));
            row.push(Span::surface("│", palette().dim, palette().tool_bg));
            row.extend(
                right
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| padded("", width - half - 1)),
            );
            framed(row)
        })
        .collect()
}

fn unified_rows(pairs: &[Pair], old: usize, new: usize, width: usize) -> Vec<Line> {
    let mut rows = Vec::new();
    for (added, mut number) in [(false, old), (true, new)] {
        for pair in pairs {
            let left = pair.left.as_deref().map(|text| text.replace('\t', "    "));
            let right = pair.right.as_deref().map(|text| text.replace('\t', "    "));
            let (a, b) = changed(
                left.as_deref().unwrap_or(""),
                right.as_deref().unwrap_or(""),
            );
            let (text, range, mark) = if added {
                (right.as_deref(), b, '+')
            } else {
                (left.as_deref(), a, '-')
            };
            if let Some(text) = text {
                rows.extend(
                    source(Some(text), number, mark, width, Some(range))
                        .into_iter()
                        .map(framed),
                );
                number += 1;
            }
        }
    }
    rows
}

fn source(
    text: Option<&str>,
    number: usize,
    mark: char,
    width: usize,
    changed: Option<Range<usize>>,
) -> Vec<Vec<Span>> {
    let Some(text) = text else {
        return vec![padded("", width)];
    };
    let normalized = text.replace('\t', "    ");
    let text = normalized.as_str();
    let gutter = format!(" {number:>4} {mark} ");
    let room = width.saturating_sub(cell_width(&gutter)).max(1);
    let (fg, bg) = match mark {
        '-' => (palette().red, palette().diff_del_bg),
        '+' => (palette().green, palette().diff_add_bg),
        _ => (palette().dim, palette().tool_bg),
    };
    let spans = code(text).into_iter().skip(1).collect::<Vec<_>>();
    Input::new(text, room)
        .rows
        .iter()
        .enumerate()
        .map(|(index, range)| {
            let label = if index == 0 {
                gutter.clone()
            } else {
                format!("{}↪ ", " ".repeat(cell_width(&gutter).saturating_sub(2)))
            };
            let mut line = vec![Span::surface(label, fg, bg)];
            let mut offset = 0;
            for span in &spans {
                let end = offset + span.text.len();
                let first = offset.max(range.start);
                let last = end.min(range.end);
                if first < last {
                    let selected = changed
                        .as_ref()
                        .map(|r| first.max(r.start)..last.min(r.end));
                    let mut boundaries = vec![first, last];
                    if let Some(r) = &selected
                        && r.start < r.end
                    {
                        boundaries.extend([r.start, r.end]);
                    }
                    boundaries.sort_unstable();
                    boundaries.dedup();
                    for part in boundaries.windows(2) {
                        let mut fragment = span.slice(part[0] - offset..part[1] - offset);
                        fragment.bg = if selected.as_ref().is_some_and(|r| r.contains(&part[0])) {
                            blend(bg, fg, 0.25)
                        } else {
                            bg
                        };
                        line.push(fragment);
                    }
                }
                offset = end;
            }
            let used = line.iter().map(Span::width).sum::<usize>();
            line.push(Span::surface(
                " ".repeat(width.saturating_sub(used)),
                fg,
                bg,
            ));
            line
        })
        .collect()
}
