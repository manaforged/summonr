use sigil::prelude::{
    ColorAtom, LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, Rgba8, Style, container,
    label,
};
use sigil_tui::cell_width;

use super::canvas::{self, Ink, Span, ellipsize, palette, shown};

mod chrome;
mod composer;
mod footer;
mod history;
mod panels;
mod surface;
pub(super) mod todos;
pub(crate) mod transcript;

pub(super) use composer::{busy_status, composer_hint, draft_view, margin};
pub(super) use history::History;
pub(super) use surface::layer;

pub(super) fn secret_row(prompt: &str, typed: usize, width: usize) -> String {
    const KEYS: &str = "   enter / esc";
    let dots = "•".repeat(typed.min(24));
    let label = if prompt.is_empty() { "secret" } else { prompt };
    let room = width.saturating_sub(cell_width(&dots) + cell_width(KEYS) + 12);
    format!("password {}  {dots}{KEYS}", ellipsize(label, room))
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Run {
    text: String,
    width: usize,
    ink: Ink,
    surface: Ink,
    bold: bool,
    italic: bool,
}

pub(super) fn run(span: &Span) -> Run {
    Run {
        text: span.text.clone(),
        width: span.width(),
        ink: canvas::ink(span.fg),
        surface: canvas::ink(span.bg),
        bold: span.bold,
        italic: span.italic,
    }
}

pub(super) fn render_background(
    runs: impl IntoIterator<Item = Run>,
    background: Rgba8,
) -> NodeBuilder {
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .height(canvas::cell().h)
                .fill_width()
                .gap(0.0),
        )
        .style(Style::new().bg(background));
    for run in runs {
        row = row.child(run_node(run));
    }
    row
}

pub(super) fn run_node(run: Run) -> NodeBuilder {
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
    container()
        .layout(
            LayoutStyle::flex_row()
                .width(run.width as f32 * metrics.w)
                .height(metrics.h),
        )
        .style(Style::new().bg(shown(run.surface, palette().base)))
        .child(
            label(run.text)
                .class(class)
                .style(Style::new().text_color(ColorAtom::Rgba8(shown(run.ink, palette().text)))),
        )
}

pub(super) fn accent(_name: &str) -> Rgba8 {
    palette().accent
}
