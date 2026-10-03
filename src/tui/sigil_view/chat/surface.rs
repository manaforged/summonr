use std::rc::Rc;

use sigil::prelude::IntoNode;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::container;
use sigil::prelude::effect;
use sigil::prelude::empty_state;
use sigil::prelude::node;
use sigil::prelude::signal;
use sigil::prelude::virtual_list;
use sigil::reactive::defer;
use sigil::reactive::untrack;

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::todos;
use super::{App, Entry, canvas};
use crate::prefs::Prefs;
use crate::tui::ToolState;

pub(in crate::tui::sigil_view) fn layer(
    app: App,
    cols: u16,
    editor: impl IntoNode + 'static,
) -> NodeBuilder {
    let width = canvas::inner_cols(usize::from(cols));
    let transcript_app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .padding_y(canvas::edge(app.viewport.peek().1))
                .gap(0.0),
        )
        .child(node(move || {
            let empty = || empty_state("Summonr", "ask anything · /login · @file · !cmd · /help");
            transcript(transcript_app, (canvas::GUTTER, canvas::cell().h), empty)
        }))
        .child(canvas::gutter(panels::agents(app.clone(), width)))
        .child(todos::widget(app.clone(), cols))
        .child(canvas::gutter(panels::bands(app.clone(), width)))
        .child(composer::widget(app.clone(), width, editor))
        .child(canvas::gutter(footer::footer_bar(app, width)))
}

pub(in crate::tui::sigil_view) fn inline_layer(
    app: App,
    cols: u16,
    editor: impl IntoNode + 'static,
) -> NodeBuilder {
    let width = canvas::inner_cols(usize::from(cols));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .gap(0.0),
        )
        .child(canvas::gutter(panels::agents(app.clone(), width)))
        .child(todos::widget(app.clone(), cols))
        .child(canvas::gutter(panels::bands(app.clone(), width)))
        .child(composer::widget(app.clone(), width, editor))
        .child(canvas::gutter(footer::footer_bar(app, width)))
}

type Content = (usize, u8, bool, Prefs);

fn content(entry: &Entry, live: bool, prefs: &Prefs) -> Content {
    let (size, state) = match entry {
        Entry::Assistant { text, done } => (text.len(), u8::from(*done)),
        Entry::Thinking(text) => (text.len(), 0),
        Entry::Tool {
            state,
            output,
            expanded,
            ..
        } => (
            output.len(),
            u8::from(!matches!(state, ToolState::Running))
                + u8::from(matches!(state, ToolState::Failed))
                + u8::from(*expanded) * 4,
        ),
        Entry::ToolGroup { calls, .. } => {
            let size: usize = calls.iter().map(|call| call.output.len()).sum();
            let mut state = 0_u8;
            for call in calls {
                state |= u8::from(!matches!(call.state, ToolState::Running));
                state |= u8::from(matches!(call.state, ToolState::Failed)) * 2;
                state |= u8::from(call.expanded) * 4;
            }
            (size, state)
        }
        Entry::User(_)
        | Entry::Receipt(_)
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Aborted(_)
        | Entry::System(_)
        | Entry::Error(_)
        | Entry::Chart(_)
        | Entry::Image { .. } => (0, 0),
    };
    (size, state, live, prefs.clone())
}

fn row(app: App, index: usize, spacing: (f32, f32)) -> NodeBuilder {
    let model = app.model;
    let version = sigil::reactive::derived(move || {
        model.with(|model| {
            let live = model.busy_since.is_some() && index + 1 == model.entries.len();
            model
                .entries
                .get(index)
                .map(|entry| content(entry, live, &model.prefs))
        })
    });
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(spacing.0)
                .padding_bottom(spacing.1),
        )
        .children_reactive(move || {
            version.with(|_| ());
            untrack(|| {
                let (entry, live, prefs) = model.with(|model| {
                    (
                        model.entries.get(index).cloned(),
                        model.busy_since.is_some() && index + 1 == model.entries.len(),
                        model.prefs.clone(),
                    )
                });
                entry
                    .map(|entry| vec![entry::row(entry, live, &prefs, &app, index)])
                    .unwrap_or_default()
            })
        })
}

pub(in crate::tui::sigil_view) fn transcript(
    app: App,
    spacing: (f32, f32),
    empty: fn() -> NodeBuilder,
) -> NodeBuilder {
    let model = app.model;
    let keys = sigil::reactive::derived(move || {
        model.with(|model| {
            Rc::new(
                (0..model.entries.len())
                    .map(|index| index as u64)
                    .collect::<Vec<u64>>(),
            )
        })
    });
    let jump = signal(0_usize);
    let target = app.search.target;
    effect(move || {
        if let Some(index) = target.get() {
            jump.set(index);
            defer(move || target.set(None));
        }
    });
    let empty_now = sigil::reactive::derived(move || keys.get().is_empty());
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            if empty_now.get() {
                return vec![empty()];
            }
            jump.set(keys.peek().len().saturating_sub(1));
            let live = app.clone();
            vec![
                container()
                    .selectable_text(true)
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .grow(1.0)
                            .min_size(0.0, 0.0),
                    )
                    .child(
                        virtual_list(keys, move |index, _: &u64| {
                            row(live.clone(), index, spacing)
                        })
                        .key_by(|_, key| *key)
                        .measured()
                        .estimate(canvas::cell().h * 2.0)
                        .viewport_fill()
                        .scroll_bind(app.scroll_y)
                        .max_scroll_bind(app.max_scroll)
                        .content_height_bind(app.content_height)
                        .follow_end(true)
                        .scrollbar(true)
                        .follow_row(jump),
                    ),
            ]
        })
}
