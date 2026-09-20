use sigil::prelude::{
    AlignItems, Dimension, JustifyContent, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder,
    NodeEventsExt, NodeStyleExt, Position, card, classes, container, kv_row, label, section_label,
    spacer,
};

use super::chrome;
use super::{App, Model, Speed};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Target {
    Context,
    Cache,
}

pub(super) fn footer_bar(app: App, _full_width: usize, width: usize) -> NodeBuilder {
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
            let mut column = container().layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .shrink(0.0)
                    .position(Position::Relative),
            );
            if show_ctx && !show_cache {
                column = column.child(meter_overlay(&model, width, Target::Context));
            } else if show_cache && !show_ctx {
                column = column.child(meter_overlay(&model, width, Target::Cache));
            }
            column = column.child(footer_row(update.clone(), &model));
            vec![column]
        })
}

fn footer_row(app: App, model: &Model) -> NodeBuilder {
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_x(16.0)
                .align_items(AlignItems::Center)
                .gap(12.0),
        )
        .child(label(app.info.workspace.clone()).class(classes!("text-xs text-gray-12")));
    if let Some(branch) = &app.git_branch {
        row = row.child(label(format!("· {branch}")).class(classes!("text-xs text-warn-11")));
    }
    row = row.child(spacer());
    if let Some(rate) = model.speed.last() {
        row = row.child(label(Speed::label(rate)).class(classes!("text-xs text-gray-11")));
    }
    row = row.child(meter_button(
        app.clone(),
        Target::Context,
        meter("ctx", chrome::context_percent(model), "bg-accent"),
    ));
    row.child(meter_button(
        app,
        Target::Cache,
        meter("cache", chrome::cache_percent(model), "bg-info-11"),
    ))
}

fn meter(caption: &str, percent: u64, fill: &'static str) -> NodeBuilder {
    let bar = 64.0_f32;
    container()
        .layout(
            LayoutStyle::flex_row()
                .align_items(AlignItems::Center)
                .gap(8.0),
        )
        .child(label(caption).class(classes!("text-xs text-gray-11")))
        .child(
            container()
                .layout(LayoutStyle::flex_row().width(bar).height(8.0))
                .class(classes!("bg-gray-1"))
                .child(
                    container()
                        .layout(
                            LayoutStyle::block()
                                .width(bar * percent.min(100) as f32 / 100.0)
                                .height(8.0),
                        )
                        .class(fill),
                ),
        )
        .child(label(format!("{percent}%")).class(classes!("text-xs text-gray-12")))
}

fn meter_button(app: App, target: Target, child: NodeBuilder) -> NodeBuilder {
    let press = app.clone();
    let enter = app.clone();
    let leave = app.clone();
    let down_target = target.clone();
    let enter_target = target.clone();
    container()
        .child(child)
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

fn meter_overlay(model: &Model, width: usize, target: Target) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .position(Position::Absolute)
                .inset_left(0.0)
                .inset_right(0.0)
                .inset_bottom(24.0)
                .padding_x(16.0)
                .justify_content(JustifyContent::End),
        )
        .child(meter_card(model, width, target))
}

fn meter_card(model: &Model, _width: usize, target: Target) -> NodeBuilder {
    let (title, rows) = match target {
        Target::Context => ("ctx", chrome::context_facts(model)),
        Target::Cache => ("cache", chrome::cache_facts(model)),
    };
    let mut body = container()
        .layout(LayoutStyle::flex_column().width(220.0).gap(4.0))
        .child(section_label(title));
    for (key, value) in rows {
        body = body.child(kv_row(key, label(value)));
    }
    container()
        .layout(
            LayoutStyle::flex_column()
                .width(220.0)
                .shrink(0.0)
                .max_size(220.0, Dimension::Auto),
        )
        .child(
            card(body).layout(
                LayoutStyle::flex_column()
                    .width(220.0)
                    .padding(8.0)
                    .gap(4.0)
                    .shrink(0.0),
            ),
        )
}
