use std::rc::Rc;

use sigil::prelude::IntoNode;
use sigil::prelude::Space;
use sigil::prelude::{
    NodeBuilder, Signal, container, effect, empty_state, fragment, node, signal, virtual_list,
};
use sigil::reactive::untrack;
use sigil::reactive::{defer, derived};

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::{App, Model, canvas};
use sigil::prelude::Widget;
use summonr_agent::prefs::{DiffLayout, ToolOutput, Visibility};

#[derive(Clone, Copy, PartialEq, Eq)]
struct RowPrefs {
    tools: ToolOutput,
    diff_layout: DiffLayout,
    reasoning: Visibility,
    receipts: Visibility,
    spacing: u8,
    split_wide: bool,
}

pub(crate) fn layer(app: App, editor: impl Fn(App) -> NodeBuilder + 'static) -> NodeBuilder {
    let width_app = app.clone();
    let width = derived(move || canvas::inner_cols(usize::from(width_app.chat_cols())));
    let transcript_app = app.clone();
    let status_app = app.clone();
    let bands_app = app.clone();
    let comp_app = app.clone();
    sigil::prelude::column()
        .fill_width()
        .fill_height()
        .min_width(0.0)
        .min_height(0.0)
        .padding_y(Space::raw(
            canvas::edge(app.viewport.peek().1),
            "one terminal cell on a roomy screen",
        ))
        .gap(Space::None)
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
        .child(pin(move || {
            canvas::gutter(panels::status_bar(status_app.clone(), width.get()))
        }))
        .child(pin(move || {
            canvas::gutter(panels::bands(bands_app.clone(), width.get()))
        }))
        .child(pin(move || {
            let _rows = comp_app.viewport.get().1;
            let edit = editor(comp_app.clone());
            composer::widget(comp_app.clone(), width.get(), edit)
        }))
        .child(container().fill_width().grow(1.0))
        .child(pin(move || {
            canvas::gutter(footer::footer_bar(app.clone(), width.get()))
        }))
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
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .grow(0.0)
        .shrink(1.0)
        .min_width(0.0)
        .min_height(0.0)
        .drive(sigil::animation::Property::Height, height)
}

fn pin(build: impl Fn() -> NodeBuilder + 'static) -> NodeBuilder {
    container()
        .fill_width()
        .shrink(0.0)
        .children_reactive(move || vec![build()])
}

pub(crate) fn inline_layer(app: App, cols: u16, editor: impl IntoNode + 'static) -> NodeBuilder {
    let width = canvas::inner_cols(usize::from(cols));
    sigil::prelude::column()
        .fill_width()
        .min_width(0.0)
        .min_height(0.0)
        .gap(Space::None)
        .child(canvas::gutter(panels::status_bar(app.clone(), width)))
        .child(canvas::gutter(panels::bands(app.clone(), width)))
        .child(composer::widget(app.clone(), width, editor))
        .child(canvas::gutter(footer::footer_bar(app, width)))
}

fn window_row(
    app: App,
    index: usize,
    spacing: (f32, f32),
    prefs: Signal<RowPrefs>,
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
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .min_width(0.0)
        .min_height(0.0)
        .children_reactive(move || {
            let ((at, _), _) = current.get();
            prefs.with(|_| ());
            open.with(|_| ());
            untrack(|| app.window.row(&app, (at, index), open, spacing))
        })
}

fn terminal_row(
    app: App,
    index: usize,
    spacing: (f32, f32),
    prefs: Signal<RowPrefs>,
) -> NodeBuilder {
    let probe = app.clone();
    let current = derived(move || probe.model.with(|model| terminal_revision(model, index)));
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .min_width(0.0)
        .min_height(0.0)
        .children_reactive(move || {
            let (_, _, stacked, _) = current.get();
            prefs.with(|_| ());
            let gap = (spacing.0, if stacked { 0.0 } else { spacing.1 });
            untrack(|| single(&app, index, gap))
        })
}

fn terminal_revision(model: &Model, index: usize) -> (entry::Revision, bool, bool, u64) {
    let live = model.busy_since.is_some() && index + 1 == model.entries.len();
    let stacked = model.entries.get(index).is_some_and(entry::subagents)
        && model.entries.get(index + 1).is_some_and(entry::subagents);
    (
        entry::revision(&model.entries, index),
        live,
        stacked,
        model.entries.stamp,
    )
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
        return sigil::prelude::row()
            .gap(sigil::prelude::Space::None)
            .align_items(sigil::prelude::AlignItems::Stretch)
            .fill_width()
            .min_width(0.0)
            .min_height(0.0)
            .padding_x(Space::raw(spacing.0, "the caller's gutter"))
            .padding_bottom(Space::raw(spacing.1, "the caller's entry spacing"))
            .justify_content(sigil::prelude::JustifyContent::Center)
            .child(
                sigil::prelude::column()
                    .gap(sigil::prelude::Space::None)
                    .fill_width()
                    .min_width(0.0)
                    .min_height(0.0)
                    .max_width(READING - 2.0 * spacing.0)
                    .child(node),
            );
    }
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .min_width(0.0)
        .min_height(0.0)
        .padding_x(Space::raw(spacing.0, "the caller's gutter"))
        .padding_bottom(Space::raw(spacing.1, "the caller's entry spacing"))
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
    let table = canvas::is_window().then(|| {
        let shape = derived(move || model.with(|model| (model.entries.len(), model.entries.stamp)));
        derived(move || {
            shape.with(|_| ());
            untrack(|| model.with(|model| Rc::<[entry::Slot]>::from(entry::slots(&model.entries))))
        })
    });
    let jump = follow(&app);
    let settle = app.settle;
    let empty_now = sigil::reactive::derived(move || keys.get().is_empty());
    let fit_app = app.clone();
    let prefs = derived(move || {
        let split_wide = entry::Fit::chat(&fit_app) == entry::Fit::Wide;
        model.with(|model| RowPrefs {
            tools: model.prefs.tools,
            diff_layout: model.prefs.diff_layout,
            reasoning: model.prefs.reasoning,
            receipts: model.prefs.receipts,
            spacing: model.prefs.spacing,
            split_wide: model.prefs.diff_layout == DiffLayout::Split && split_wide,
        })
    });
    fragment().children_reactive(move || {
        if empty_now.get() {
            return vec![
                sigil::prelude::column()
                    .gap(sigil::prelude::Space::None)
                    .fill_width()
                    .grow(1.0)
                    .min_width(0.0)
                    .min_height(0.0)
                    .child(empty()),
            ];
        }
        jump.set(keys.peek().len().saturating_sub(1));
        settle.set(true);
        let live = app.clone();
        let copied = app.clone();
        vec![
            sigil::prelude::column()
                .selectable_text(true)
                .selection_copy(move |text, result| copied.copy_notice(text, result))
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .grow(1.0)
                .min_width(0.0)
                .min_height(0.0)
                .child(
                    virtual_list(keys, move |index, _: &u64| match table {
                        Some(table) => window_row(live.clone(), index, spacing, prefs, table),
                        None => terminal_row(live.clone(), index, spacing, prefs),
                    })
                    .key_by(|_, key| *key)
                    .measured()
                    .estimate(canvas::cell().h * 2.0)
                    .fill()
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
