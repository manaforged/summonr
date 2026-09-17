use std::ops::Range;

use sigil::prelude::{
    ColorAtom, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Rgba8, Style, container,
    label,
};
use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

use super::canvas::{self, blend, palette};
use super::model::Agent;

mod chrome;
mod composer;
mod grid;
mod history;
mod surface;
pub(crate) mod transcript;

pub(super) use canvas::{Cell, rgb};
pub(super) use composer::{
    FOOTER_GAP, FOOTER_PAD, FOOTER_ROWS, INPUT_HEIGHTS, body_rows, busy_status, composer_hint,
    draft_view, input_rows, margin, overlay_rows, prompt_row,
};
pub(super) use history::History;
pub(super) use surface::{footer_bar, layer};

#[derive(Clone)]
pub(super) struct Span {
    text: String,
    fg: Rgba8,
    bg: Rgba8,
    bold: bool,
    italic: bool,
    source: Option<Source>,
}

#[derive(Clone)]
enum Source {
    Text(Range<usize>),
    Block(Range<usize>),
}

impl Source {
    fn range(&self) -> &Range<usize> {
        match self {
            Self::Text(range) | Self::Block(range) => range,
        }
    }
}

impl Span {
    fn text(text: impl Into<String>) -> Self {
        Self::styled(text, palette().text)
    }

    fn styled(text: impl Into<String>, fg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: palette().base,
            bold: false,
            italic: false,
            source: None,
        }
    }

    fn surface(text: impl Into<String>, fg: Rgba8, bg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
            bold: false,
            italic: false,
            source: None,
        }
    }

    fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    fn width(&self) -> usize {
        cell_width(self.text.as_str())
    }

    fn source(mut self, start: usize) -> Self {
        self.source = Some(Source::Text(start..start + self.text.len()));
        self
    }

    fn slice(&self, range: Range<usize>) -> Self {
        let source = match &self.source {
            Some(Source::Text(source)) => Some(Source::Text(
                source.start + range.start..source.start + range.end,
            )),
            _ => self.source.clone(),
        };
        Self {
            text: self.text[range].to_string(),
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
            italic: self.italic,
            source,
        }
    }
}

pub(in crate::tui::sigil_view) struct Line {
    spans: Vec<Span>,
    bg: Rgba8,
}

impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self {
            spans,
            bg: palette().base,
        }
    }
}

pub(super) struct ApprovalRow {
    pub text: String,
    pub allow: (usize, usize),
    pub always: Option<(usize, usize)>,
    pub deny: (usize, usize),
}

pub(super) fn secret_row(prompt: &str, typed: usize, width: usize) -> String {
    const KEYS: &str = "   enter / esc";
    let dots = "•".repeat(typed.min(24));
    let label = if prompt.is_empty() { "secret" } else { prompt };
    let room = width.saturating_sub(cell_width(&dots) + cell_width(KEYS) + 12);
    format!("password {}  {dots}{KEYS}", ellipsize(label, room))
}

pub(super) fn approval_row(detail: &str, width: usize, persist: bool) -> ApprovalRow {
    const ALLOW: &str = "[ allow ]";
    const ALWAYS: &str = "[ always ]";
    const DENY: &str = "[ deny ]";
    let extra = if persist { cell_width(ALWAYS) + 1 } else { 0 };
    let room = width.saturating_sub(cell_width(ALLOW) + cell_width(DENY) + extra + 18);
    let mut text = format!("approve {}   ", ellipsize(detail, room));
    let allow_from = cell_width(&text);
    text.push_str(ALLOW);
    let allow_to = cell_width(&text);
    let always = persist.then(|| {
        text.push(' ');
        let from = cell_width(&text);
        text.push_str(ALWAYS);
        (from, cell_width(&text))
    });
    text.push(' ');
    let deny_from = cell_width(&text);
    text.push_str(DENY);
    let deny_to = cell_width(&text);
    text.push_str(if persist { "   y / Y / n" } else { "   y / n" });
    ApprovalRow {
        text,
        allow: (allow_from, allow_to),
        always,
        deny: (deny_from, deny_to),
    }
}

pub(in crate::tui::sigil_view) fn agent_strip_rows(agents: &[Agent], hidden: bool) -> u16 {
    let live = agents.iter().filter(|agent| !agent.done).count();
    if hidden || live == 0 {
        0
    } else {
        (live.min(6) + 2) as u16
    }
}

pub(super) fn write_at(
    cells: &mut [Cell],
    width: usize,
    left: usize,
    row: usize,
    text: &str,
    fg: Rgba8,
) {
    write_line_at(
        cells,
        width,
        left,
        width.saturating_sub(left),
        row,
        &[Span::styled(text, fg)],
    );
}

pub(super) fn ellipsize(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut shown = text.to_string();
    truncate(&mut shown, width.saturating_sub(1));
    shown.push('…');
    shown
}

fn write_line_at(
    cells: &mut [Cell],
    stride: usize,
    left: usize,
    width: usize,
    y: usize,
    line: &[Span],
) {
    let start = y.saturating_mul(stride).saturating_add(left);
    let Some(row) = cells.get_mut(start..start.saturating_add(width)) else {
        return;
    };
    let mut x = 0;
    for span in line {
        for text in span.text.graphemes(true) {
            let step = cell_width(text);
            if x >= width || x.saturating_add(step) > width {
                return;
            }
            if step == 0 {
                continue;
            }
            row[x] = Cell {
                text: text.into(),
                fg: span.fg,
                bg: span.bg,
                bold: span.bold,
                italic: span.italic,
            };
            for pad in 1..step {
                if x + pad < width {
                    row[x + pad] = Cell {
                        text: "".into(),
                        fg: span.fg,
                        bg: span.bg,
                        bold: span.bold,
                        italic: span.italic,
                    };
                }
            }
            x += step;
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Run {
    text: String,
    width: usize,
    fg: Rgba8,
    bg: Rgba8,
    bold: bool,
    italic: bool,
}

fn runs(cells: &[Cell]) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < cells.len() {
        let cell = &cells[start];
        let mut end = start + 1;
        while end < cells.len()
            && cells[end].fg == cell.fg
            && cells[end].bg == cell.bg
            && cells[end].bold == cell.bold
            && cells[end].italic == cell.italic
        {
            end += 1;
        }
        runs.push(Run {
            text: cells[start..end]
                .iter()
                .map(|cell| cell.text.as_str())
                .collect(),
            width: end - start,
            fg: cell.fg,
            bg: cell.bg,
            bold: cell.bold,
            italic: cell.italic,
        });
        start = end;
    }
    runs
}

pub(super) fn row(cells: &[Cell]) -> NodeBuilder {
    render(runs(cells))
}

fn render(runs: impl IntoIterator<Item = Run>) -> NodeBuilder {
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .height(canvas::cell().h)
                .fill_width()
                .gap(0.0),
        )
        .style(Style::new().bg(palette().base));
    for run in runs {
        let metrics = canvas::cell();
        let class = match (run.bold, run.italic, metrics.text.is_empty()) {
            (true, true, true) => "font-mono font-bold italic",
            (true, false, true) => "font-mono font-bold",
            (false, true, true) => "font-mono italic",
            (false, false, true) => "font-mono",
            (true, true, false) => "font-mono font-bold italic text-sm",
            (true, false, false) => "font-mono font-bold text-sm",
            (false, true, false) => "font-mono italic text-sm",
            (false, false, false) => "font-mono text-sm",
        };
        row = row.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .width(run.width as f32 * metrics.w)
                        .height(metrics.h),
                )
                .style(Style::new().bg(run.bg))
                .child(
                    label(run.text)
                        .class(class)
                        .style(Style::new().text_color(ColorAtom::Rgba8(run.fg))),
                ),
        );
    }
    row
}

fn truncate(text: &mut String, width: usize) {
    let mut used = 0;
    let end = text.grapheme_indices(true).find_map(|(index, grapheme)| {
        used += cell_width(grapheme);
        (used > width).then_some(index)
    });
    if let Some(end) = end {
        text.truncate(end);
    }
}

pub(super) fn accent(_name: &str) -> Rgba8 {
    palette().accent
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
