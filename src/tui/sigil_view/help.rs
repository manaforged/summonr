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
}

impl View {
    pub(super) fn new() -> Self {
        Self {
            open: signal(false),
            query: signal(String::new()),
        }
    }

    pub(super) fn show(self) {
        self.query.set(String::new());
        self.open.set(true);
    }
}

pub(super) fn layer(app: App) -> NodeBuilder {
    let query = app.help.query;
    let keys = app.clone();
    let (cols, rows) = app.viewport.peek();
    let visible = canvas::overlay_shown(usize::MAX, rows).saturating_sub(1) as f32;
    let rows = derived(move || {
        matching(
            &crate::tui::help_text(&keys.model.get().prefs.hotkeys),
            &query.get(),
        )
    });
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
                    .height(canvas::overlay_field()),
            )
            .child(
                virtual_list(rows, move |_, line: &String| {
                    pick(false, line.clone(), String::new())
                })
                .row_height(canvas::overlay_list_row())
                .height(canvas::overlay_list_row() * visible)
                .viewport_height(canvas::overlay_list_row() * visible),
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
