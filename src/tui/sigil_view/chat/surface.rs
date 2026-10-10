use std::rc::Rc;

use sigil::prelude::IntoNode;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, container, effect, empty_state,
    fragment, node, signal, virtual_list,
};
use sigil::reactive::untrack;
use sigil::reactive::{defer, derived};
use sigil::style::{NodeStyleExt, NumericAtomKind};

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::todos;
use super::{App, Entry, Model, canvas};
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
        .child(fit(app.clone()).child(node(move || {
            let hint_app = transcript_app.clone();
            transcript(
                transcript_app,
                (canvas::GUTTER, canvas::cell().h),
                move || {
                    let _redraw = hint_app.model.get();
                    let hint = if hint_app.unseated() {
                        "ask anything · /login · @file · !cmd · /help"
                    } else {
                        "ask anything · @file · !cmd · /help"
                    };
                    empty_state(env!("CARGO_PKG_NAME"), hint)
                },
            )
        })))
        .child(pin(canvas::gutter(panels::agents(app.clone(), width))))
        .child(pin(todos::widget(app.clone(), cols)))
        .child(pin(canvas::gutter(panels::bands(app.clone(), width))))
        .child(pin(composer::widget(app.clone(), width, editor)))
        .child(container().layout(LayoutStyle::block().fill_width().grow(1.0)))
        .child(pin(canvas::gutter(footer::footer_bar(app, width))))
}

fn fit(app: App) -> NodeBuilder {
    let model = app.model;
    let content = app.content_height;
    let viewport = app.viewport;
    let height = derived(move || {
        let measured = content.get();
        if measured > 0.0 && !model.with(|model| model.entries.is_empty()) {
            measured
        } else {
            canvas::cell().h * f32::from(viewport.get().1)
        }
    });
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(0.0)
                .shrink(1.0)
                .min_size(0.0, 0.0),
        )
        .animate(NumericAtomKind::Height, height)
}

fn pin(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().shrink(0.0))
        .child(child)
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

pub(in crate::tui::sigil_view) type Content = (usize, u8, bool, Prefs);

pub(in crate::tui::sigil_view) fn content(entry: &Entry, live: bool, prefs: &Prefs) -> Content {
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
    #[cfg(feature = "gui")]
    if canvas::is_window() {
        return super::super::desktop::entry::row(app, index, spacing);
    }
    let probe = app.clone();
    let current =
        sigil::reactive::derived(move || probe.model.with(|model| slot_marks(model, index)));
    container()
        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
        .children_reactive(move || {
            let (at, _) = current.get();
            untrack(|| match at {
                entry::Slot::Member => vec![
                    container().layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0)),
                ],
                entry::Slot::Lead(range) if range.end - range.start > 1 => {
                    vec![inset(group(&app, range), spacing)]
                }
                entry::Slot::Lead(_) | entry::Slot::Single => single(&app, index, spacing),
            })
        })
}

fn slot_marks(model: &Model, index: usize) -> (entry::Slot, Vec<Content>) {
    let at = entry::slot(&model.entries, index);
    let range = match &at {
        entry::Slot::Lead(range) => range.clone(),
        entry::Slot::Single => index..index + 1,
        entry::Slot::Member => index..index,
    };
    let last = model.entries.len();
    let live = model.busy_since.is_some();
    let mark: Vec<_> = model
        .entries
        .get(range.clone())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(offset, item)| content(item, live && range.start + offset + 1 == last, &model.prefs))
        .collect();
    (at, mark)
}

fn group(app: &App, range: std::ops::Range<usize>) -> NodeBuilder {
    let rows = app.model.with(|model| {
        let last = model.entries.len();
        let prefs = model.prefs.clone();
        let live_turn = model.busy_since.is_some();
        range
            .map(|at| {
                (
                    at,
                    model.entries.get(at).cloned(),
                    live_turn && at + 1 == last,
                    prefs.clone(),
                )
            })
            .collect::<Vec<_>>()
    });
    let mut column = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .min_size(0.0, 0.0)
            .gap(0.0),
    );
    for (at, item, live, prefs) in rows {
        if let Some(item) = item {
            column = column.child(entry::row(item, live, &prefs, app, at));
        }
    }
    column
}

fn single(app: &App, index: usize, spacing: (f32, f32)) -> Vec<NodeBuilder> {
    let (item, live, prefs) = app.model.with(|model| {
        (
            model.entries.get(index).cloned(),
            model.busy_since.is_some() && index + 1 == model.entries.len(),
            model.prefs.clone(),
        )
    });
    item.filter(|item| !entry::hidden(item, &prefs))
        .map(|item| vec![inset(entry::row(item, live, &prefs, app, index), spacing)])
        .unwrap_or_default()
}

fn inset(node: NodeBuilder, spacing: (f32, f32)) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(spacing.0)
                .padding_bottom(spacing.1),
        )
        .child(node)
}

pub(in crate::tui::sigil_view) fn transcript(
    app: App,
    spacing: (f32, f32),
    empty: impl Fn() -> NodeBuilder + 'static,
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
    let settle = app.settle;
    let scroll = app.scroll_y;
    let bottom = app.max_scroll;
    effect(move || {
        let end = bottom.get();
        if untrack(|| settle.get()) {
            defer(move || scroll.set(end));
        }
    });
    let empty_now = sigil::reactive::derived(move || keys.get().is_empty());
    fragment().children_reactive(move || {
        if empty_now.get() {
            return vec![
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .grow(1.0)
                            .min_size(0.0, 0.0),
                    )
                    .child(empty()),
            ];
        }
        jump.set(keys.peek().len().saturating_sub(1));
        settle.set(true);
        let live = app.clone();
        let copied = app.clone();
        vec![
            container()
                .selectable_text(true)
                .on_selection_copy(move |text, result| copied.copy_notice(text, result))
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
