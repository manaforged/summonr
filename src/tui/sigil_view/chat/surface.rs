use sigil::prelude::{
    AlignItems, ColorAtom, Cx, IntoNode, JustifyContent, LayoutBuilderExt, LayoutReactiveExt,
    LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt, Style, classes, container,
    effect, empty_state, label, node, scroll, signal, spacer,
};

use super::composer;
use super::entry;
use super::footer;
use super::panels;
use super::todos;
use super::{App, Entry, canvas};
use crate::tui::short_session;

pub(in crate::tui::sigil_view) fn layer(
    app: App,
    cols: u16,
    editor: impl IntoNode + 'static,
) -> NodeBuilder {
    let full_width = usize::from(cols);
    let width = content_width(cols);
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
            cx.add(transcript(transcript_app.clone()))
        }))
        .child(panels::agents(app.clone(), full_width, width))
        .child(todos::widget(app.clone(), cols))
        .child(panels::bands(app.clone(), full_width, width))
        .child(composer::widget(app.clone(), full_width, width, editor))
        .child(footer::footer_bar(app, full_width, width))
}

fn content_width(cols: u16) -> usize {
    let width = usize::from(cols);
    width.saturating_sub(super::margin(width) * 2)
}

fn header(app: App) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(16.0)
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

fn transcript(app: App) -> NodeBuilder {
    let live = app.clone();
    let bounds = signal((0.0, 0.0, 0.0, 0.0));
    let viewport = signal((0.0, 0.0));
    let transcript_height = app.transcript_height;
    let extent = app.extent;
    let body = container()
        .bounds_reactive(bounds)
        .on_added(move |_, _| {
            live.tui_chat_active.set(true);
            effect(move || {
                let height = bounds.get().3;
                if height > 0.0 {
                    extent.set(height);
                }
            });
        })
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .fill_height()
                .grow(1.0)
                .min_size(0.0, 0.0)
                .justify_content(JustifyContent::End)
                .padding_x(16.0)
                .padding_y(8.0)
                .gap(0.0),
        )
        .children_reactive({
            let live = app.clone();
            move || {
                let _revision = live.panes.get();
                let _ticks = live.ticks.get();
                let entries: Vec<Entry> = live.model.borrow().entries.to_vec();
                let tools = live.model.borrow().prefs.tools;
                let layout = live.model.borrow().prefs.diff_layout;
                if entries.is_empty() {
                    return vec![empty_state(
                        "Summonr",
                        "ask anything · /login setup · @file attaches · !cmd runs · /help",
                    )];
                }
                let mut column = container().layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .gap(canvas::cell().h),
                );
                for (index, item) in entries.into_iter().enumerate() {
                    let node = entry::row(item, tools, layout, &live);
                    if live.search.target.peek() != Some(index) {
                        column = column.child(node);
                        continue;
                    }
                    let measured = signal((0.0, 0.0, 0.0, 0.0));
                    let target = live.search.target;
                    let scroll = live.scroll_y;
                    column = column.child(node.bounds_reactive(measured).on_added(move |_, _| {
                        effect(move || {
                            let (_, y, _, height) = measured.get();
                            let (_, top, _, body_height) = bounds.get();
                            if target.peek() == Some(index) && height > 0.0 && body_height > 0.0 {
                                scroll.set((y - top).max(0.0));
                                target.set(None);
                            }
                        });
                    }));
                }
                vec![column]
            }
        });
    let select = app.clone();
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
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Left) {
                select.selection_anchor.set(None);
                event.stop_propagation();
            }
        })
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
