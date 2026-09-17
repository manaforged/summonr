use std::rc::Rc;

use sigil::prelude::{
    Cx, IntoNode, LayoutBuilderExt, LayoutStyle, ModifiersState, MouseButton, NodeBuilder,
    NodeEventsExt, ObjectFit, Point, Position, container, image, node,
};

use super::super::{app::App, hit, model::Model, suggest, view};
use super::chrome::{cache_meter, context_meter, line_width, status};
use super::composer::composer_offset;
use super::grid::{grid, stamp_menu};
use super::{
    Cell, FOOTER_GAP, FOOTER_PAD, FOOTER_ROWS, Run, accent, agent_strip_rows, approval_row,
    body_rows, canvas, margin, overlay_rows, prompt_row, render, runs, transcript, write_line_at,
};

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Row(usize, Rc<[Run]>),
    Image {
        index: usize,
        source: u64,
        left: usize,
        top: usize,
        cols: usize,
        rows: usize,
        crop: (usize, usize),
    },
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let press = app.clone();
    let drag = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset_left(0.0)
                .inset_right(0.0)
                .inset_top(0.0)
                .inset_bottom(f32::from(FOOTER_GAP + FOOTER_PAD + FOOTER_ROWS) * canvas::cell().h)
                .fill_width()
                .fill_height()
                .gap(0.0),
        )
        .pointer_down(move |event| {
            if event.button != Some(MouseButton::Left) {
                return;
            }
            if let Some(token) = approval_hit(&press, event.position) {
                press.approval_answer(token);
                event.stop_propagation();
                return;
            }
            let cell = canvas::cell();
            let col = (event.position.x / cell.w).max(0.0) as usize;
            let row = (event.position.y / cell.h).max(0.0) as usize;
            match press.hits.at(col, row) {
                Some(target @ (hit::Target::Context | hit::Target::Cache)) => {
                    press.toggle_meter(target);
                    event.stop_propagation();
                    return;
                }
                Some(hit::Target::AgentsClose) => {
                    press.hide_agents();
                    event.stop_propagation();
                    return;
                }
                _ => {}
            }
            if let Some(caret) = composer_offset(&press, event.position) {
                press.place(caret, event.modifiers.contains(ModifiersState::SHIFT));
                event.capture_pointer();
                event.stop_propagation();
            }
        })
        .pointer_move(move |event| {
            let cell = canvas::cell();
            let col = (event.position.x / cell.w).max(0.0) as usize;
            let row = (event.position.y / cell.h).max(0.0) as usize;
            drag.meter_hover.set(match drag.hits.at(col, row) {
                Some(hit::Target::Context | hit::Target::Cache) => drag.hits.at(col, row),
                _ => None,
            });
            if event.is_captured()
                && let Some(caret) = composer_offset(&drag, event.position)
            {
                drag.place(caret, true);
                event.stop_propagation();
            }
        })
        .children_keyed(move || {
            view::watch(&app);
            let (_, rows) = app.viewport.get();
            let cols = app.chat_cols();
            let rows = overlay_rows(rows);
            let draft = app.draft.get();
            let attached = app.attached.get();
            let mut cells = grid(&app.model.borrow(), &app, &draft, cols, rows, attached);
            if let Some(menu) = suggest::menu(&app, cols, rows) {
                stamp_menu(&mut cells, usize::from(cols).max(1), &menu);
            }
            let mut nodes = cells
                .chunks(usize::from(cols).max(1))
                .enumerate()
                .map(|(index, cells)| keyed(index, cells))
                .collect::<Vec<_>>();
            nodes.extend(pictures(&app.model.borrow(), &app, cols, rows));
            nodes
        })
}

pub(in crate::tui::sigil_view) fn footer_bar(app: App) -> NodeBuilder {
    let press = app.clone();
    let drag = app.clone();
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h),
        )
        .pointer_down(move |event| {
            if event.button != Some(MouseButton::Left) {
                return;
            }
            let cell = canvas::cell();
            let col = (event.position.x / cell.w).max(0.0) as usize;
            let row = (event.position.y / cell.h).max(0.0) as usize;
            if let Some(target @ (hit::Target::Context | hit::Target::Cache)) =
                press.hits.at(col, row)
            {
                press.toggle_meter(target);
                event.stop_propagation();
            }
        })
        .pointer_move(move |event| {
            let cell = canvas::cell();
            let col = (event.position.x / cell.w).max(0.0) as usize;
            let row = (event.position.y / cell.h).max(0.0) as usize;
            drag.meter_hover.set(match drag.hits.at(col, row) {
                Some(hit::Target::Context | hit::Target::Cache) => drag.hits.at(col, row),
                _ => None,
            });
        })
        .children_keyed(move || {
            view::watch(&app);
            let cols = app.chat_cols();
            let width = usize::from(cols);
            let model = app.model.borrow();
            let left = margin(width);
            let content = width.saturating_sub(left * 2);
            let accent = accent(&model.prefs.theme);
            let mut cells = vec![Cell::blank(); width];
            if width == 0 {
                return Vec::new();
            }
            write_line_at(
                &mut cells,
                width,
                left,
                content,
                0,
                &status(&model, &app, content, accent),
            );
            let ctx = context_meter(&model, content);
            let cache = cache_meter(&model, content.saturating_sub(line_width(&ctx) + 2));
            let cache_w = line_width(&cache);
            let ctx_w = line_width(&ctx);
            let right = left + content;
            let (_, rows) = app.viewport.peek();
            let footer_row = usize::from(overlay_rows(rows) + FOOTER_GAP);
            if cache_w > 0 {
                app.hits.add(
                    footer_row,
                    right.saturating_sub(cache_w),
                    right,
                    hit::Target::Cache,
                );
            }
            if ctx_w > 0 {
                let ctx_to =
                    right.saturating_sub(cache_w.saturating_add(2 * usize::from(cache_w > 0)));
                app.hits.add(
                    footer_row,
                    ctx_to.saturating_sub(ctx_w),
                    ctx_to,
                    hit::Target::Context,
                );
            }
            vec![keyed(0, &cells)]
        })
}

fn keyed(index: usize, cells: &[Cell]) -> (Key, Box<dyn IntoNode>) {
    let runs: Rc<[Run]> = runs(cells).into();
    let key = Key::Row(index, Rc::clone(&runs));
    (
        key,
        node(move |cx: &mut Cx| cx.add(render(runs.iter().cloned()))),
    )
}

fn pictures(model: &Model, app: &App, cols: u16, rows: u16) -> Vec<(Key, Box<dyn IntoNode>)> {
    let width = usize::from(cols);
    if width == 0 || usize::from(rows) < 5 {
        return Vec::new();
    }
    let content_left = margin(width);
    let body_height = usize::from(body_rows(
        cols,
        rows,
        &app.draft.peek(),
        model.busy_since.is_some(),
    ))
    .saturating_sub(usize::from(agent_strip_rows(
        &model.agents,
        model.agents_hidden,
    )));
    let history = app.history.borrow();
    let max_start = history.lines.len().saturating_sub(body_height);
    let start = ((app.scroll_y.peek() / canvas::cell().h) as usize).min(max_start);
    let lift = 0;
    let cell = canvas::cell();
    history
        .images
        .iter()
        .enumerate()
        .filter_map(|(index, placed)| {
            let top = placed.line;
            let first = top.max(start);
            let last = top
                .saturating_add(placed.rows)
                .min(start.saturating_add(body_height));
            if placed.rows == 0 || last <= first {
                return None;
            }
            let hidden = first - top;
            let shown = last - first;
            let fitted = transcript::paint_source(&placed.source, placed.cols, placed.rows);
            let key = Key::Image {
                index,
                source: fitted.id(),
                left: content_left,
                top: 2 + lift + first - start,
                cols: placed.cols,
                rows: shown,
                crop: (hidden, placed.rows),
            };
            let pixels = fitted.height() as usize;
            let mut picture = image(fitted)
                .fit(ObjectFit::Contain)
                .layout(LayoutStyle::block().fill_width().fill_height());
            if shown < placed.rows {
                let crop_top = hidden * pixels / placed.rows;
                let crop_height = (shown * pixels / placed.rows).max(1);
                picture = picture.crop(
                    u32::try_from(crop_top).unwrap_or(u32::MAX),
                    u32::try_from(crop_height).unwrap_or(u32::MAX),
                );
            }
            Some((
                key,
                node(
                    container()
                        .layout(
                            LayoutStyle::block()
                                .position(Position::Absolute)
                                .inset_left(content_left as f32 * cell.w)
                                .inset_top((2 + lift + first - start) as f32 * cell.h)
                                .width(placed.cols as f32 * cell.w)
                                .height(shown as f32 * cell.h),
                        )
                        .child(picture),
                ),
            ))
        })
        .collect()
}

fn approval_hit(app: &App, point: Point) -> Option<&'static str> {
    let (detail, persist) = app.approval.peek()?;
    let cols = app.chat_cols();
    let width = usize::from(cols);
    let rows = app.viewport.peek().1;
    if width == 0 || rows < 5 {
        return None;
    }
    let content_left = margin(width);
    let content_width = width.saturating_sub(content_left * 2);
    let prompt = usize::from(prompt_row(cols, rows, &app.draft.peek())).saturating_sub(1);
    let row = approval_row(&detail, content_width, persist);
    let cell = canvas::cell();
    let col = (point.x / cell.w).max(0.0) as usize;
    let line = (point.y / cell.h).max(0.0) as usize;
    if line != prompt {
        return None;
    }
    if (content_left + row.allow.0..content_left + row.allow.1).contains(&col) {
        return Some("y");
    }
    if let Some((from, to)) = row.always
        && (content_left + from..content_left + to).contains(&col)
    {
        return Some("always");
    }
    if (content_left + row.deny.0..content_left + row.deny.1).contains(&col) {
        return Some("n");
    }
    None
}
