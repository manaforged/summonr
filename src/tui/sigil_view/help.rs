use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Signal, container, derived, label, modal, signal,
    text_field, virtual_list,
};

use super::app::App;

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
            &super::super::help_text(&keys.model.borrow().prefs.hotkeys),
            &query.get(),
        )
    });
    container().layout(LayoutStyle::block()).child(
        modal(app.help.open)
            .title("Help · keys and commands")
            .trap(true)
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .gap(6.0)
                            .fill_width()
                            .fill_height(),
                    )
                    .child(text_field(query).placeholder("find"))
                    .child(
                        virtual_list(rows, |_, row: &String| {
                            container().child(label(row.clone()))
                        })
                        .row_height(18.0)
                        .viewport_fill(),
                    ),
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
