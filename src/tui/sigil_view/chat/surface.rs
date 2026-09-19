use sigil::prelude::{
    Cx, IntoNode, LayoutBuilderExt, LayoutReactiveExt, LayoutStyle, MouseButton, NodeBuilder,
    NodeEventsExt, NodeStyleExt, ObjectFit, Position, Style, container, effect, image, node,
    scroll, signal,
};
use sigil_tui::cell_width;

use super::super::app::App;
use super::super::canvas::{self, Span, palette};
use super::chrome;
use super::composer;
use super::footer;
use super::history::Placed;
use super::panels;
use super::todos;
use super::{accent, margin, render_background, run, run_node};

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
        .child(header(app.clone(), full_width, width))
        .child(node(move |cx: &mut Cx| {
            cx.add(transcript(transcript_app, full_width, width))
        }))
        .child(panels::agents(app.clone(), full_width, width))
        .child(todos::widget(app.clone(), cols))
        .child(panels::bands(app.clone(), full_width, width))
        .child(composer::widget(app.clone(), full_width, width, editor))
        .child(footer::footer_bar(app, full_width, width))
}

fn content_width(cols: u16) -> usize {
    let width = usize::from(cols);
    width.saturating_sub(margin(width) * 2)
}

fn inset(width: usize, child: NodeBuilder) -> NodeBuilder {
    let left = margin(width);
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h)
                .padding_x(left as f32 * canvas::cell().w),
        )
        .child(child)
}

pub(super) fn line(
    app: App,
    spans: Vec<Span>,
    background: sigil::prelude::Rgba8,
    width: usize,
) -> NodeBuilder {
    if spans.iter().all(|span| span.diff_layout.is_none()) {
        return inset(width, render_background(spans.iter().map(run), background));
    }
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h)
                .gap(0.0),
        )
        .style(Style::new().bg(background));
    for span in &spans {
        let mut node = run_node(run(span));
        if let Some(layout) = span.diff_layout {
            let press = app.clone();
            node = node.pointer_down(move |event| {
                if event.button == Some(MouseButton::Left) {
                    press.set_diff_layout(layout);
                    event.stop_propagation();
                }
            });
        }
        row = row.child(node);
    }
    inset(width, row)
}

fn header(app: App, full_width: usize, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h)
                .shrink(0.0),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let model = update.model.borrow();
            vec![line(
                update.clone(),
                chrome::header(&model, update.fps.get(), width, accent(&model.prefs.theme)),
                palette().base,
                full_width,
            )]
        })
}

fn transcript(app: App, full_width: usize, width: usize) -> NodeBuilder {
    let bounds = signal((0.0, 0.0, 0.0, 0.0));
    let total = signal(0.0_f32);
    let measure = app.clone();
    let select = app.clone();
    let body_app = app.clone();
    let body = container()
        .layout(
            LayoutStyle::flex_column()
                .fill()
                .min_size(0.0, 0.0)
                .position(Position::Relative),
        )
        .children_reactive(move || transcript_rows(&body_app, full_width, width, total));
    let scroll_body = scroll(body, total)
        .fill()
        .scroll_bind(app.scroll_y)
        .auto_pin_to_bottom(false);
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .grow(1.0)
                .min_size(0.0, 0.0),
        )
        .bounds_reactive(bounds)
        .on_added(move |_, _| {
            measure.tui_chat_active.set(true);
            effect(move || {
                let height = bounds.get().3;
                if measure.transcript_height.peek() != height {
                    measure.transcript_height.set(height);
                    measure.reflow();
                }
            });
        })
        .pointer_down(move |event| {
            if event.button != Some(MouseButton::Left) {
                return;
            }
            let cell = canvas::cell();
            let col = (event.window_position.x / cell.w).max(0.0) as u16;
            let row = (event.window_position.y / cell.h).max(0.0) as u16;
            select.selection_anchor.set(Some((col, row)));
            event.stop_propagation();
        })
        .child(scroll_body)
}

fn transcript_rows(
    app: &App,
    full_width: usize,
    width: usize,
    total: sigil::prelude::Signal<f32>,
) -> Vec<NodeBuilder> {
    let _panes = app.panes.get();
    let _scroll_y = app.scroll_y.get();
    let _transcript_height = app.transcript_height.get();
    let model = app.model.borrow();
    let stamp = model.entries.stamp;
    let mut history = app.history.borrow_mut();
    let rows = history.lines(&model, width, stamp);
    let height = canvas::cell().h;
    total.set(rows.len() as f32 * height);
    let viewport = app.transcript_height.peek().max(height);
    let start = (app.scroll_y.peek() / height).floor() as usize;
    let end = (start + (viewport / height).ceil() as usize + 2).min(rows.len());
    let mut nodes = rows
        .iter()
        .enumerate()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|(index, row)| {
            let child = line(app.clone(), row.spans.clone(), row.bg, full_width);
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset_top(index as f32 * height)
                        .inset_left(0.0)
                        .inset_right(0.0)
                        .height(height),
                )
                .child(child)
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        let hint = composer::empty_hint(width);
        let pad = width.saturating_sub(cell_width(hint)) / 2;
        let text = format!("{}{}", " ".repeat(pad), hint);
        nodes.push(line(
            app.clone(),
            vec![Span::styled(text, palette().dim)],
            palette().base,
            full_width,
        ));
        total.set(canvas::cell().h);
    }
    nodes.extend(images(&history.images, full_width, start, end));
    nodes
}

fn images(placed: &[Placed], width: usize, start: usize, end: usize) -> Vec<NodeBuilder> {
    let cell = canvas::cell();
    placed
        .iter()
        .filter(|picture| picture.line < end && picture.line.saturating_add(picture.rows) > start)
        .map(|picture| {
            let source = super::transcript::fit_source(&picture.source, picture.cols, picture.rows);
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset_left(margin(width) as f32 * cell.w)
                        .inset_top(picture.line as f32 * cell.h)
                        .width(picture.cols as f32 * cell.w)
                        .height(picture.rows as f32 * cell.h),
                )
                .hittable(false)
                .child(
                    image(source)
                        .fit(ObjectFit::Contain)
                        .layout(LayoutStyle::block().fill_width().fill_height()),
                )
        })
        .collect()
}
