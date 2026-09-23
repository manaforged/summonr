use sigil::prelude::{
    AlignItems, ColorAtom, Cx, IntoNode, LayoutBuilderExt, LayoutReactiveExt, LayoutStyle,
    MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, Signal, Style, classes, container,
    effect, empty_state, label, node, scroll, signal, spacer,
};

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::todos;
use super::{App, Entry, canvas};
use crate::tui::{ToolState, short_session};

pub(in crate::tui::sigil_view) fn layer(
    app: App,
    cols: u16,
    editor: impl IntoNode + 'static,
) -> NodeBuilder {
    let width = usize::from(cols);
    let transcript_app = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .min_size(0.0, 0.0)
                .gap(0.0),
        )
        .child(header(app.clone()))
        .child(node(move |cx: &mut Cx| {
            transcript_app.tui_chat_active.set(true);
            let select = transcript_app.clone();
            let layout = LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(16.0)
                .gap(canvas::cell().h);
            let empty = || {
                empty_state(
                    "Summonr",
                    "ask anything · /login setup · @file attaches · !cmd runs · /help",
                )
            };
            cx.add(
                transcript(transcript_app.clone(), layout, empty).pointer_down(move |event| {
                    if event.button == Some(MouseButton::Left) {
                        select.selection_anchor.set(None);
                        event.stop_propagation();
                    }
                }),
            )
        }))
        .child(panels::agents(app.clone(), width))
        .child(todos::widget(app.clone(), cols))
        .child(panels::bands(app.clone(), width))
        .child(composer::widget(app.clone(), width, editor))
        .child(footer::footer_bar(app, width))
}

fn header(app: App) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .align_items(AlignItems::Center)
                .shrink(0.0)
                .gap(8.0),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let fps = update.fps.get();
            let model = update.model.borrow();
            let mode = if model.snapshot.plan {
                "plan"
            } else if model.snapshot.work {
                "work"
            } else if model.snapshot.goal {
                "goal"
            } else {
                "auto"
            };
            let mode_label = label(mode.to_string());
            let mode_label = if model.snapshot.plan {
                mode_label.class(classes!("text-xs text-accent"))
            } else if model.snapshot.work {
                mode_label.class(classes!("text-xs text-warn-11"))
            } else if model.snapshot.goal {
                mode_label.class(classes!("text-xs text-error-11"))
            } else {
                mode_label.class(classes!("text-xs text-success-11"))
            };
            let mut row: Vec<NodeBuilder> =
                vec![
                    container().child(label("summonr").class(classes!("text-sm font-bold")).style(
                        Style::new().text_color(ColorAtom::Rgba8(canvas::palette().accent)),
                    )),
                    container().child(
                        label(model.snapshot.model.clone()).class(classes!("text-sm text-gray-12")),
                    ),
                ];
            if let Some(relay) = model
                .snapshot
                .relay
                .as_deref()
                .filter(|relay| !relay.is_empty())
            {
                row.push(
                    container()
                        .child(label(format!("· {relay}")).class(classes!("text-xs text-gray-11"))),
                );
            }
            row.push(spacer());
            row.push(
                container().child(
                    label(format!("ses {}", short_session(&model.session)))
                        .class(classes!("text-xs text-gray-11")),
                ),
            );
            row.push(container().child(mode_label));
            if fps > 0 {
                row.push(
                    container()
                        .child(label(format!("{fps} fps")).class(classes!("text-xs text-gray-11"))),
                );
            }
            row
        })
}

type Key = (u64, usize, usize, u8, bool);

fn key(entry: &Entry, index: usize, stamp: u64, flags: (bool, bool)) -> Key {
    let (size, state) = match entry {
        Entry::Assistant { text, done } => (text.len(), u8::from(*done)),
        Entry::Thinking(text) => (text.len(), u8::from(flags.0)),
        Entry::Tool { state, output, .. } => (
            output.len(),
            u8::from(!matches!(state, ToolState::Running))
                + u8::from(matches!(state, ToolState::Failed)),
        ),
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
    (stamp, index, size, state, flags.1)
}

fn build(
    app: App,
    index: usize,
    bounds: Signal<(f32, f32, f32, f32)>,
    target: bool,
) -> Box<dyn IntoNode> {
    node(move |cx: &mut Cx| {
        let (entry, prefs) = {
            let model = app.model.borrow();
            (model.entries.get(index).cloned(), model.prefs.clone())
        };
        let Some(entry) = entry else {
            return cx.add(container());
        };
        let row = entry::row(entry, &prefs, &app);
        if !target {
            return cx.add(row);
        }
        let measured = signal((0.0, 0.0, 0.0, 0.0));
        let target = app.search.target;
        let scroll = app.scroll_y;
        cx.add(row.bounds_reactive(measured).on_added(move |_, _| {
            effect(move || {
                let (_, y, _, height) = measured.get();
                let (_, top, _, body_height) = bounds.get();
                if target.peek() == Some(index) && height > 0.0 && body_height > 0.0 {
                    scroll.set((y - top).max(0.0));
                    target.set(None);
                }
            });
        }))
    })
}

pub(in crate::tui::sigil_view) fn transcript(
    app: App,
    layout: LayoutStyle,
    empty: fn() -> NodeBuilder,
) -> NodeBuilder {
    let bounds = signal((0.0, 0.0, 0.0, 0.0));
    let viewport = signal((0.0, 0.0));
    let transcript_height = app.transcript_height;
    let extent = app.extent;
    let live = app.clone();
    let body = container()
        .bounds_reactive(bounds)
        .on_added(move |_, _| {
            effect(move || {
                let height = bounds.get().3;
                if height > 0.0 {
                    extent.set(height);
                }
            });
        })
        .layout(layout)
        .children_keyed(move || {
            let _revision = live.panes.get();
            let target = live.search.target.peek();
            let model = live.model.borrow();
            let entries = &model.entries;
            if entries.is_empty() {
                return vec![((entries.stamp, usize::MAX, 0, 0, false), node(empty()))];
            }
            let busy = model.busy_since.is_some();
            let last = entries.len() - 1;
            entries
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let aimed = target == Some(index);
                    let flags = (busy && index == last, aimed);
                    let row = build(live.clone(), index, bounds, aimed);
                    (key(item, index, entries.stamp, flags), row)
                })
                .collect()
        });
    container()
        .size_reactive(viewport)
        .on_added(move |_, _| {
            effect(move || {
                let height = viewport.get().1;
                if height > 0.0 {
                    transcript_height.set(height);
                }
            });
        })
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .child(
            scroll(body, app.extent)
                .fill()
                .scroll_bind(app.scroll_y)
                .auto_pin_to_bottom(app.search.target.peek().is_none()),
        )
}

#[cfg(test)]
pub(crate) fn visible_range(
    scroll_y: f32,
    cell_h: f32,
    rows: usize,
    viewport: f32,
) -> (usize, usize) {
    if rows == 0 || cell_h <= 0.0 {
        return (0, 0);
    }
    let viewport = viewport.max(cell_h);
    let visible = (viewport / cell_h).ceil() as usize + 2;
    let raw = scroll_y / cell_h;
    let start = if raw.is_finite() {
        raw.max(0.0).floor() as usize
    } else {
        rows.saturating_sub(visible.max(1))
    };
    let start = start.min(rows.saturating_sub(visible.max(1)));
    let end = start.saturating_add(visible).min(rows);
    (start, end)
}

#[cfg(test)]
pub(crate) fn pin_extent(content: f32, viewport: f32, cell_h: f32) -> f32 {
    (content - viewport.max(cell_h)).max(0.0)
}

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
