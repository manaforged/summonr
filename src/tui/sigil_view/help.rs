use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Signal, WidgetLayoutExt, container, derived,
    signal, text_field, virtual_list,
};

use super::app::App;
use super::canvas;
use super::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct View {
    pub(super) open: Signal<bool>,
    pub(super) query: Signal<String>,
    focus: Signal<u64>,
    scroll: Signal<f32>,
}

impl View {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            query: signal(String::new()),
            focus: signal(0),
            scroll: signal(0.0),
        }
    }

    pub(super) fn show(self) {
        self.query.set(String::new());
        self.scroll.set(0.0);
        self.focus.set(self.focus.peek() + 1);
        self.open.set(true);
    }
}

pub(super) fn shift(app: &App, delta: isize) {
    let total = lines(app, &app.help.query.peek()).len();
    let row = canvas::overlay_list_row();
    let max = total.saturating_sub(visible(app)) as f32 * row;
    let next = delta as f32 * row + app.help.scroll.peek();
    app.help.scroll.set(next.clamp(0.0, max));
}

fn visible(app: &App) -> usize {
    canvas::overlay_shown(usize::MAX, app.viewport.peek().1).saturating_sub(1)
}

fn lines(app: &App, query: &str) -> Vec<String> {
    matching(
        &crate::tui::help_text(&app.model.get().prefs.hotkeys),
        query,
    )
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let View {
        query,
        focus,
        scroll,
        ..
    } = app.help;
    let keys = app.clone();
    let cols = app.viewport.peek().0;
    let visible = visible(&app) as f32;
    let rows = derived(move || lines(&keys, &query.get()));
    shell(
        app.help.open,
        "Help",
        cols,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .child(
                text_field(query)
                    .placeholder("find")
                    .focus_when(focus)
                    .change(move |_| scroll.set(0.0))
                    .height(canvas::overlay_field()),
            )
            .child(
                virtual_list(rows, move |_, line: &String| {
                    pick(false, line.clone(), String::new())
                })
                .row_height(canvas::overlay_list_row())
                .height(canvas::overlay_list_row() * visible)
                .viewport_height(canvas::overlay_list_row() * visible)
                .scroll_bind(scroll),
            )
            .child(hint(
                "type to filter · scroll for more · esc close",
                String::new(),
            )),
    )
}

fn matching(text: &str, query: &str) -> Vec<String> {
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| term.to_lowercase())
        .collect();
    text.lines()
        .filter(|line| {
            let line = line.to_lowercase();
            terms.iter().all(|term| line.contains(term))
        })
        .map(str::to_string)
        .collect()
}
