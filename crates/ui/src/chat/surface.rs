use std::rc::Rc;

use sigil::prelude::IntoNode;
use sigil::prelude::Space;
use sigil::prelude::{
    LayoutStyle, NodeBuilder, Signal, container, effect, empty_state, fragment, node, signal,
    virtual_list,
};
use sigil::reactive::untrack;
use sigil::reactive::{defer, derived};

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::{App, Model, canvas};
use sigil::prelude::Widget;
use summonr_agent::prefs::Prefs;

pub(crate) fn layer(app: App, cols: u16, editor: impl IntoNode + 'static) -> NodeBuilder {
    let width = canvas::inner_cols(usize::from(cols));
    let transcript_app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .padding_y(Space::raw(
                    canvas::edge(app.viewport.peek().1),
                    "one terminal cell on a roomy screen",
                ))
                .gap(Space::None),
        )
        .child(fit(app.clone()).child(node(move || {
            let hint_app = transcript_app.clone();
            transcript(
                transcript_app,
                (canvas::GUTTER, canvas::cell().h),
                move || {
                    let probe = hint_app.clone();
                    let hint = derived(move || {
                        probe.model.with(|_| ());
                        if probe.unconfigured() {
                            "ask anything · /login · @file · !cmd · /help"
                        } else {
                            "ask anything · @file · !cmd · /help"
                        }
                    });
                    empty_state(crate::PRODUCT, hint)
                },
            )
        })))
        .child(pin(canvas::gutter(panels::status_bar(app.clone(), width))))
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
        .animate_height(height)
}

fn pin(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fill_width().shrink(0.0))
        .child(child)
}

pub(crate) fn inline_layer(app: App, cols: u16, editor: impl IntoNode + 'static) -> NodeBuilder {
    let width = canvas::inner_cols(usize::from(cols));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .gap(Space::None),
        )
        .child(canvas::gutter(panels::status_bar(app.clone(), width)))
        .child(canvas::gutter(panels::bands(app.clone(), width)))
        .child(composer::widget(app.clone(), width, editor))
        .child(canvas::gutter(footer::footer_bar(app, width)))
}

fn row(
    app: App,
    index: usize,
    spacing: (f32, f32),
    prefs: Signal<Prefs>,
    table: Signal<Rc<[entry::Slot]>>,
) -> NodeBuilder {
    let probe = app.clone();
    let current = derived(move || {
        let at = table.with(|table| table.get(index).cloned().unwrap_or(entry::Slot::Single));
        probe
            .model
            .with(|model| (slot_revisions(model, index, at), model.entries.stamp))
    });
    let open: Signal<Option<bool>> = signal(None);
    container()
        .layout(LayoutStyle::flex_column().fill_width().min_size(0.0, 0.0))
        .children_reactive(move || {
            let ((at, _), _) = current.get();
            prefs.with(|_| ());
            if canvas::is_window() {
                open.with(|_| ());
                return untrack(|| app.window.row(&app, (at, index), open, spacing));
            }
            untrack(|| match at {
                entry::Slot::Member => Vec::new(),
                entry::Slot::Lead(range) if range.end - range.start > 1 => {
                    vec![inset(group(&app, range), spacing)]
                }
                entry::Slot::Lead(_) | entry::Slot::Single => single(&app, index, spacing),
            })
        })
}

fn slot_revisions(
    model: &Model,
    index: usize,
    at: entry::Slot,
) -> (entry::Slot, Vec<(entry::Revision, bool)>) {
    let range = at.span(index);
    let last = model.entries.len();
    let live = model.busy_since.is_some();
    let revisions: Vec<_> = model
        .entries
        .get(range.clone())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(offset, _)| {
            (
                entry::revision(&model.entries, range.start + offset),
                live && range.start + offset + 1 == last,
            )
        })
        .collect();
    (at, revisions)
}

fn group(app: &App, range: std::ops::Range<usize>) -> NodeBuilder {
    let stacked = app
        .model
        .with(|model| model.entries.get(range.start).is_some_and(entry::subagents));
    let (prefs, rows) = app.model.with(|model| {
        let last = model.entries.len();
        let live_turn = model.busy_since.is_some();
        let rows = range
            .map(|at| {
                (
                    at,
                    model.entries.get(at).cloned(),
                    live_turn && at + 1 == last,
                )
            })
            .collect::<Vec<_>>();
        (model.prefs.clone(), rows)
    });
    let mut column = container().layout(
        LayoutStyle::flex_column()
            .fill_width()
            .min_size(0.0, 0.0)
            .gap(if stacked {
                Space::None
            } else {
                Space::raw(canvas::cell().h, "one terminal cell between entries")
            }),
    );
    for (at, item, live) in rows {
        if let Some(item) = item {
            column = column.child(entry::row(item, live, &prefs, app, at));
        }
    }
    column
}

fn single(app: &App, index: usize, spacing: (f32, f32)) -> Vec<NodeBuilder> {
    let (item, live, prefs) = app.model.with(|model| model.entry_at(index));
    item.filter(|item| !entry::hidden(item, &prefs))
        .map(|item| vec![inset(entry::row(item, live, &prefs, app, index), spacing)])
        .unwrap_or_default()
}

/// The widest the window's reading column grows, gutters included.
pub(crate) const READING: f32 = 704.0;

pub(crate) fn inset(node: NodeBuilder, spacing: (f32, f32)) -> NodeBuilder {
    if canvas::is_window() {
        return container()
            .layout(
                LayoutStyle::flex_row()
                    .fill_width()
                    .min_size(0.0, 0.0)
                    .padding_x(Space::raw(spacing.0, "the caller's gutter"))
                    .padding_bottom(Space::raw(spacing.1, "the caller's entry spacing"))
                    .justify_content(sigil::prelude::JustifyContent::Center),
            )
            .child(
                container()
                    .layout(
                        LayoutStyle::flex_column()
                            .fill_width()
                            .min_size(0.0, 0.0)
                            .max_width(READING - 2.0 * spacing.0),
                    )
                    .child(node),
            );
    }
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(Space::raw(spacing.0, "the caller's gutter"))
                .padding_bottom(Space::raw(spacing.1, "the caller's entry spacing")),
        )
        .child(node)
}

fn follow(app: &App) -> Signal<usize> {
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
    jump
}

pub(crate) fn transcript(
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
    let shape = derived(move || model.with(|model| (model.entries.len(), model.entries.stamp)));
    let table = derived(move || {
        shape.with(|_| ());
        untrack(|| model.with(|model| Rc::<[entry::Slot]>::from(entry::slots(&model.entries))))
    });
    let jump = follow(&app);
    let settle = app.settle;
    let empty_now = sigil::reactive::derived(move || keys.get().is_empty());
    let prefs = derived(move || model.with(|model| model.prefs.clone()));
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
                        row(live.clone(), index, spacing, prefs, table)
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

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
