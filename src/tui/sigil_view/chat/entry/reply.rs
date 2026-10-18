use sigil::kernel::{Cx, IntoNode, NodeId};
use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Radius, Signal, Token,
    container, markdown, text,
};
use sigil::reactive::derived;
use sigil::widgets::DiffBuilder;

use super::{patch_of, patch_path, patch_view};
use crate::prefs::DiffLayout;
use crate::tui::sigil_view::chat::App;
use crate::tui::sigil_view::look::Look;

const FENCE: &str = "```";

#[derive(Clone, PartialEq, Eq, Hash)]
struct StatLine {
    path: String,
    added: u32,
    removed: u32,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Shown {
    Patch {
        path: Option<String>,
        patch: String,
    },
    Stat {
        rows: Vec<StatLine>,
        summary: Option<String>,
    },
}

#[derive(Clone, PartialEq)]
enum Part {
    Prose(String),
    Shown(Shown),
}

struct Fence {
    opener: String,
    lines: Vec<String>,
}

enum Piece {
    Prose(Signal<Vec<Part>>, usize),
    Shown(Box<(Shown, App, DiffLayout)>),
}

impl IntoNode for Piece {
    fn build(self, cx: &mut Cx) -> NodeId {
        match self {
            Self::Prose(list, index) => markdown(derived(move || {
                list.with(|list| match list.get(index) {
                    Some(Part::Prose(text)) => text.clone(),
                    _ => String::new(),
                })
            }))
            .build(cx),
            Self::Shown(shown) => {
                let (shown, app, layout) = *shown;
                paint(shown, &app, layout).build(cx)
            }
        }
    }
}

pub(super) fn body(source: Signal<String>, app: &App) -> NodeBuilder {
    let layout = app.model.peek().prefs.diff_layout;
    let list = derived(move || source.with(|text| parts(text)));
    let app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(8.0)
                .min_size(0.0, 0.0),
        )
        .children_keyed(move || {
            list.with(|parts| {
                parts
                    .iter()
                    .enumerate()
                    .map(|(index, part)| match part {
                        Part::Prose(_) => ((index, None), Piece::Prose(list, index)),
                        Part::Shown(shown) => (
                            (index, Some(fingerprint(shown))),
                            Piece::Shown(Box::new((shown.clone(), app.clone(), layout))),
                        ),
                    })
                    .collect::<Vec<_>>()
            })
        })
}

fn fingerprint(shown: &Shown) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    shown.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn stat_node_of(output: &str) -> Option<NodeBuilder> {
    let (rows, summary) = stats_of(output)?;
    Some(stat_node(&rows, summary.as_deref()))
}

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) fn diff_card(app: &App, text: &str) -> Option<NodeBuilder> {
    let layout = app.model.get().prefs.diff_layout;
    classify(text).map(|shown| paint(shown, app, layout))
}

fn paint(shown: Shown, app: &App, layout: DiffLayout) -> NodeBuilder {
    match shown {
        Shown::Patch { path, patch } => container().child(patch_node(app, layout, path, patch)),
        Shown::Stat { rows, summary } => stat_node(&rows, summary.as_deref()),
    }
}

fn patch_node(app: &App, layout: DiffLayout, path: Option<String>, patch: String) -> DiffBuilder {
    let view = patch_view(app, patch, layout, super::Fit::chat(app)).bg(Token::Field);
    match path {
        Some(path) => view.path(path),
        None => view,
    }
}

fn stat_node(rows: &[StatLine], summary: Option<&str>) -> NodeBuilder {
    let mut column = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(0.0)
                .min_size(0.0, 0.0),
        )
        .bg(Token::Field)
        .border_w(1)
        .border_color(Token::Border)
        .rounded(Radius::Md);
    for row in rows {
        column = column.child(stat_head(row));
    }
    if let Some(summary) = summary {
        column = column.child(
            Look::xs(Token::Gray11)
                .on(text(summary.to_string()))
                .nowrap()
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .padding_x(8.0)
                        .padding_bottom(6.0)
                        .min_size(0.0, 0.0),
                ),
        );
    }
    column
}

fn stat_head(row: &StatLine) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(8.0)
                .min_size(0.0, 22.0)
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .child(
            Look::xs(Token::Gray12)
                .mono()
                .bold()
                .on(text(row.path.clone()))
                .nowrap()
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
        )
        .child(
            Look::xs(Token::Success11)
                .mono()
                .on(text(format!("+{}", row.added)))
                .nowrap(),
        )
        .child(
            Look::xs(Token::Error11)
                .mono()
                .on(text(format!("−{}", row.removed)))
                .nowrap(),
        )
}

fn parts(body: &str) -> Vec<Part> {
    let mut out = Vec::new();
    let mut prose = Vec::new();
    let mut fence = None;
    for line in body.lines() {
        fence = match fence.take() {
            Some(open) => step_fence(open, line, &mut prose, &mut out),
            None if is_fence(line) => Some(Fence {
                opener: line.to_string(),
                lines: Vec::new(),
            }),
            None => {
                prose.push(line.to_string());
                None
            }
        };
    }
    if let Some(open) = fence {
        prose.push(open.opener);
        prose.extend(open.lines);
    }
    push_prose(&mut prose, &mut out);
    out
}

fn step_fence(
    mut open: Fence,
    line: &str,
    prose: &mut Vec<String>,
    out: &mut Vec<Part>,
) -> Option<Fence> {
    if !is_fence(line) {
        open.lines.push(line.to_string());
        return Some(open);
    }
    let body = open.lines.join("\n");
    if let Some(shown) = classify(&body) {
        push_prose(prose, out);
        out.push(Part::Shown(shown));
    } else {
        prose.push(open.opener);
        prose.extend(open.lines);
        prose.push(line.to_string());
    }
    None
}

fn push_prose(prose: &mut Vec<String>, out: &mut Vec<Part>) {
    if prose.is_empty() {
        return;
    }
    let text = prose.join("\n");
    prose.clear();
    if !text.trim().is_empty() {
        out.push(Part::Prose(text));
    }
}

fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with(FENCE)
}

fn classify(body: &str) -> Option<Shown> {
    if let Some(patch) = patch_of(body) {
        return Some(Shown::Patch {
            path: patch_path(body),
            patch,
        });
    }
    let (rows, summary) = stats_of(body)?;
    Some(Shown::Stat { rows, summary })
}

fn stats_of(body: &str) -> Option<(Vec<StatLine>, Option<String>)> {
    let mut rows = Vec::new();
    let mut summary = None;
    for line in body.lines() {
        let line = line.trim();
        if skipped(line) {
            continue;
        }
        if let Some(row) = stat_line(line) {
            rows.push(row);
            continue;
        }
        if summary.is_none() && summary_line(line) {
            summary = Some(line.to_string());
            continue;
        }
        return None;
    }
    (!rows.is_empty()).then_some((rows, summary))
}

fn skipped(line: &str) -> bool {
    line.is_empty()
        || line.starts_with('$')
        || line == "---"
        || line
            .trim_start_matches('$')
            .trim()
            .starts_with("summonr diff")
}

fn summary_line(line: &str) -> bool {
    let Some((count, rest)) = line.split_once(" file") else {
        return false;
    };
    !count.is_empty() && count.chars().all(|ch| ch.is_ascii_digit()) && rest.contains(" vs ")
}

fn stat_line(line: &str) -> Option<StatLine> {
    let rest = line.strip_prefix('+')?;
    let (added, rest) = number(rest)?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('−').or_else(|| rest.strip_prefix('-'))?;
    let (removed, rest) = number(rest)?;
    let trimmed = rest.trim();
    let path = trimmed.strip_suffix("  new").unwrap_or(trimmed).trim();
    if path.is_empty() || path.contains(' ') {
        return None;
    }
    Some(StatLine {
        path: path.to_string(),
        added,
        removed,
    })
}

fn number(text: &str) -> Option<(u32, &str)> {
    let end = text
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    let value = text[..end].parse().ok()?;
    Some((value, &text[end..]))
}

#[cfg(test)]
#[path = "reply_tests.rs"]
mod tests;
