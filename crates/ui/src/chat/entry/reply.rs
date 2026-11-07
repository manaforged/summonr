use sigil::prelude::Space;
use sigil::prelude::{
    AlignItems, LayoutStyle, NodeBuilder, Paint, Radius, Signal, Token, container, markdown, text,
};
use sigil::reactive::derived;
use sigil::widgets::DiffBuilder;
use sigil::{Cx, NodeId, prelude::IntoNode};

use super::{patch_of, patch_path, patch_view};
use crate::chat::App;
use crate::text_style::TextStyle;
use sigil::prelude::Widget;
use summonr_agent::diff::Stat;
use summonr_agent::prefs::DiffLayout;

const FENCE: &str = "```";

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) enum Shown {
    Patch {
        path: Option<String>,
        patch: String,
    },
    Stat {
        rows: Vec<Stat>,
        summary: Option<String>,
    },
}

#[derive(Clone, PartialEq)]
pub(crate) enum Part {
    Prose(String),
    Code {
        lang: String,
        body: String,
        closed: bool,
    },
    Shown(Shown),
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
                    Some(Part::Code { lang, body, closed }) => {
                        let end = if *closed { FENCE } else { "" };
                        format!("{FENCE}{lang}\n{body}\n{end}")
                    }
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
    let stream = std::cell::RefCell::new(Stream::default());
    let list = derived(move || source.with(|text| stream.borrow_mut().parts(text)));
    let app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(Space::Sm)
                .min_size(0.0, 0.0),
        )
        .children_keyed(move || {
            list.with(|parts| {
                parts
                    .iter()
                    .enumerate()
                    .map(|(index, part)| match part {
                        Part::Prose(_) | Part::Code { .. } => {
                            ((index, None), Piece::Prose(list, index))
                        }
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

pub(crate) fn paint(shown: Shown, app: &App, layout: DiffLayout) -> NodeBuilder {
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

fn stat_node(rows: &[Stat], summary: Option<&str>) -> NodeBuilder {
    let mut column = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(Space::None)
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
            TextStyle::xs(Token::Gray11)
                .on(text(summary.to_string()))
                .nowrap()
                .layout(
                    LayoutStyle::block()
                        .fill_width()
                        .padding_x(Space::Sm)
                        .padding_bottom(Space::raw(6.0, "between the small and medium steps"))
                        .min_size(0.0, 0.0),
                ),
        );
    }
    column
}

fn stat_head(row: &Stat) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(Space::Sm)
                .min_size(0.0, 22.0)
                .align_items(AlignItems::Center)
                .gap(Space::Sm),
        )
        .child(
            TextStyle::xs(Token::Gray12)
                .mono()
                .bold()
                .on(text(row.path.clone()))
                .nowrap()
                .ellipsize()
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
        )
        .child(
            TextStyle::xs(Token::Success11)
                .mono()
                .on(text(format!("+{}", row.added)))
                .nowrap(),
        )
        .child(
            TextStyle::xs(Token::Error11)
                .mono()
                .on(text(format!("−{}", row.removed)))
                .nowrap(),
        )
}

#[cfg(any(test, feature = "gui"))]
pub(crate) fn parts(body: &str) -> Vec<Part> {
    let mut parse = Parse::default();
    parse.feed(body);
    parse.finish()
}

#[derive(Clone)]
struct Fence {
    lang: String,
    lines: Vec<String>,
}

#[derive(Clone, Default)]
struct Parse {
    out: Vec<Part>,
    prose: Option<String>,
    fence: Option<Fence>,
}

#[derive(Default)]
struct Stream {
    seen: String,
    parse: Parse,
}

impl Stream {
    fn parts(&mut self, text: &str) -> Vec<Part> {
        if !text.starts_with(self.seen.as_str()) {
            *self = Self::default();
        }
        let whole = text.rfind('\n').map_or(0, |end| end + 1);
        if whole > self.seen.len() {
            let fresh = &text[self.seen.len()..whole];
            self.parse.feed(fresh);
            self.seen.push_str(fresh);
        }
        let mut tail = self.parse.clone();
        tail.feed(&text[self.seen.len()..]);
        tail.finish()
    }
}

impl Parse {
    fn feed(&mut self, text: &str) {
        for line in text.lines() {
            self.fence = match self.fence.take() {
                Some(open) => self.step_fence(open, line),
                None if is_fence(line) => Some(Fence {
                    lang: line.trim_start().trim_start_matches('`').trim().to_string(),
                    lines: Vec::new(),
                }),
                None => {
                    self.prose_line(line);
                    None
                }
            };
        }
    }

    fn step_fence(&mut self, mut open: Fence, line: &str) -> Option<Fence> {
        if !is_fence(line) {
            open.lines.push(line.to_string());
            return Some(open);
        }
        let body = open.lines.join("\n");
        self.push_prose();
        self.out.push(match classify(&body) {
            Some(shown) => Part::Shown(shown),
            None => Part::Code {
                lang: open.lang,
                body,
                closed: true,
            },
        });
        None
    }

    fn prose_line(&mut self, line: &str) {
        match &mut self.prose {
            Some(text) => {
                text.push('\n');
                text.push_str(line);
            }
            None => self.prose = Some(line.to_string()),
        }
    }

    fn push_prose(&mut self) {
        if let Some(text) = self.prose.take()
            && !text.trim().is_empty()
        {
            self.out.push(Part::Prose(text));
        }
    }

    fn finish(mut self) -> Vec<Part> {
        if let Some(open) = self.fence.take() {
            self.push_prose();
            self.out.push(Part::Code {
                lang: open.lang,
                body: open.lines.join("\n"),
                closed: false,
            });
        }
        self.push_prose();
        self.out
    }
}

fn is_fence(line: &str) -> bool {
    line.trim_start().starts_with(FENCE)
}

pub(crate) fn classify(body: &str) -> Option<Shown> {
    if let Some(patch) = patch_of(body) {
        return Some(Shown::Patch {
            path: patch_path(body),
            patch,
        });
    }
    let (rows, summary) = stats_of(body)?;
    Some(Shown::Stat { rows, summary })
}

fn stats_of(body: &str) -> Option<(Vec<Stat>, Option<String>)> {
    let mut rows = Vec::new();
    let mut summary = None;
    for line in body.lines().map(str::trim) {
        if line.is_empty()
            || line.starts_with('$')
            || line == "---"
            || line.starts_with("summonr diff")
            || line.starts_with("summonr session diff")
        {
            continue;
        }
        if let Ok(row) = line.parse() {
            rows.push(row);
        } else if summary.is_none()
            && line.split_once(" file").is_some_and(|(count, rest)| {
                !count.is_empty()
                    && count.bytes().all(|byte| byte.is_ascii_digit())
                    && rest.contains(" vs ")
            })
        {
            summary = Some(line.to_string());
        } else {
            return None;
        }
    }
    (!rows.is_empty()).then_some((rows, summary))
}

#[cfg(test)]
#[path = "reply_tests.rs"]
mod tests;
