use sigil::prelude::{
    JustifyContent, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt,
    NodeStyleExt, Style, container,
};
use sigil_tui::cell_width;

use super::super::app::App;
use super::super::canvas::{self, Span, palette};
use super::super::hit::Target;
use super::super::model::Model;
use super::chrome;
use super::{accent, render_background, run};

pub(super) fn footer_bar(app: App, full_width: usize, width: usize) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            drop(update.status.get());
            let _panes = update.panes.get();
            let _context_open = update.context_open.get();
            let _cache_open = update.cache_open.get();
            drop(update.meter_hover.get());
            let model = update.model.borrow();
            let show_ctx =
                update.context_open.peek() || update.meter_hover.peek() == Some(Target::Context);
            let show_cache =
                update.cache_open.peek() || update.meter_hover.peek() == Some(Target::Cache);
            let mut column =
                container().layout(LayoutStyle::flex_column().fill_width().shrink(0.0));
            if show_ctx && !show_cache {
                column = column.child(card(&model, full_width, width, Target::Context));
            } else if show_cache && !show_ctx {
                column = column.child(card(&model, full_width, width, Target::Cache));
            }
            column = column.child(footer_row(update.clone(), &model, full_width, width));
            vec![column]
        })
}

fn footer_row(app: App, model: &Model, full_width: usize, width: usize) -> NodeBuilder {
    let context = chrome::context_meter(model, width);
    let cache = chrome::cache_meter(
        model,
        width.saturating_sub(chrome::line_width(&context) + 2),
    );
    let context_width = chrome::line_width(&context);
    let cache_width = chrome::line_width(&cache);
    let meters_width =
        context_width + cache_width + usize::from(context_width > 0 && cache_width > 0) * 2;
    let speed = chrome::idle_speed(model).filter(|speed| meters_width + speed.width() < width);
    let speed_width = speed.as_ref().map_or(0, Span::width);
    let right = meters_width + speed_width;
    let left_width = width.saturating_sub(right + 1);
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(canvas::cell().h)
                .padding_x(super::margin(full_width) as f32 * canvas::cell().w),
        )
        .child(
            render_background(
                chrome::status_left(model, &app, left_width, accent(&model.prefs.theme))
                    .iter()
                    .map(run),
                palette().base,
            )
            .layout(
                LayoutStyle::flex_row()
                    .width(left_width as f32 * canvas::cell().w)
                    .height(canvas::cell().h),
            ),
        )
        .child(container().layout(LayoutStyle::flex_row().grow(1.0).height(canvas::cell().h)));
    if context_width > 0 {
        row = row.child(meter_button(app.clone(), Target::Context, context));
    }
    if cache_width > 0 {
        if context_width > 0 {
            row = row.child(container().layout(LayoutStyle::block().width(2.0 * canvas::cell().w)));
        }
        row = row.child(meter_button(app, Target::Cache, cache));
    }
    if let Some(speed) = speed {
        row = row.child(
            render_background([speed].iter().map(run), palette().base).layout(
                LayoutStyle::flex_row()
                    .width(speed_width as f32 * canvas::cell().w)
                    .height(canvas::cell().h),
            ),
        );
    }
    row
}

fn meter_button(app: App, target: Target, spans: Vec<Span>) -> NodeBuilder {
    let press = app.clone();
    let enter = app.clone();
    let leave = app.clone();
    let down_target = target.clone();
    let enter_target = target.clone();
    let width = chrome::line_width(&spans);
    render_background(spans.iter().map(run), palette().base)
        .layout(
            LayoutStyle::flex_row()
                .width(width as f32 * canvas::cell().w)
                .height(canvas::cell().h),
        )
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Left) {
                press.toggle_meter(down_target.clone());
                event.stop_propagation();
            }
        })
        .pointer_enter(move |_| enter.meter_hover.set(Some(enter_target.clone())))
        .pointer_leave(move |_| {
            if leave.meter_hover.peek() == Some(target.clone()) {
                leave.meter_hover.set(None);
            }
        })
}

fn card(model: &Model, full_width: usize, width: usize, target: Target) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(super::margin(full_width) as f32 * canvas::cell().w)
                .justify_content(JustifyContent::End),
        )
        .child(meter_card(model, width, target))
}

fn meter_card(model: &Model, width: usize, target: Target) -> NodeBuilder {
    let (title, lines) = match target {
        Target::Context => (" ctx ", chrome::context_card(model)),
        Target::Cache => (" cache ", chrome::cache_card(model)),
    };
    let card_width = width.min(30);
    let mut card = container()
        .layout(LayoutStyle::flex_column().width(card_width as f32 * canvas::cell().w))
        .style(Style::new().bg(palette().tool_bg));
    card = card.child(render_background(
        [Span::surface(
            format!(
                "╭{}{}╮",
                title,
                "─".repeat(card_width.saturating_sub(cell_width(title) + 2))
            ),
            palette().gray,
            palette().tool_bg,
        )]
        .iter()
        .map(run),
        palette().tool_bg,
    ));
    for mut row in lines {
        let used = chrome::line_width(&row);
        row.insert(0, Span::surface("│ ", palette().gray, palette().tool_bg));
        row.push(Span::surface(
            format!("{}│", " ".repeat(card_width.saturating_sub(used + 3))),
            palette().gray,
            palette().tool_bg,
        ));
        card = card.child(render_background(row.iter().map(run), palette().tool_bg));
    }
    card.child(render_background(
        [Span::surface(
            format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))),
            palette().gray,
            palette().tool_bg,
        )]
        .iter()
        .map(run),
        palette().tool_bg,
    ))
}
