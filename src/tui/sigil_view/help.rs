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
    let rows = derived(move || {
        matching(
            &crate::tui::help_text(&keys.model.borrow().prefs.hotkeys),
            &query.get(),
        )
    });
    shell(
        app.help.open,
        "Help",
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
                virtual_list(rows, |_, line: &String| {
                    pick(false, false, line.clone(), String::new())
                })
                .row_height(canvas::overlay_list_row())
                .height(canvas::overlay_list_row() * 8.0)
                .viewport_height(canvas::overlay_list_row() * 8.0),
            ),
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
