use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, LengthPercentageAuto, MouseButton, NodeBuilder,
    NodeEventsExt, NodeStyleExt, Position, classes, container, label, spacer,
};

use super::chrome;
use super::{App, Model, Speed, canvas, cell_width};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Target {
    Context,
    Cache,
}

pub(super) fn footer_bar(app: App, width: usize) -> NodeBuilder {
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
            let _context_open = update.context_open.get();
            let _cache_open = update.cache_open.get();
            drop(update.meter_hover.get());
            let model = update.model.get();
            vec![footer_row(update.clone(), &model, width)]
        })
}

fn footer_row(app: App, model: &Model, width: usize) -> NodeBuilder {
    let speed = model.speed.last().map(Speed::label);
    let taken = 42
        + app
            .git_branch
            .as_deref()
            .map_or(0, |branch| cell_width(branch) + 4)
        + speed.as_deref().map_or(0, |rate| cell_width(rate) + 2);
    let place = crate::text::ellipsize(&app.info.workspace, width.saturating_sub(taken));
    let mut row = container().layout(
        LayoutStyle::flex_row()
            .fill_width()
            .align_items(AlignItems::Center)
            .gap(12.0),
    );
    if !place.is_empty() {
        row = row.child(label(place).class(classes!("text-xs text-gray-12")));
    }
    if let Some(branch) = &app.git_branch {
        row = row.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .align_items(AlignItems::Center)
                        .shrink(0.0)
                        .gap(8.0),
                )
                .child(label("⎇").class(classes!("text-xs text-gray-11")))
                .child(label(branch.clone()).class(classes!("text-xs text-warn-11"))),
        );
    }
    row = row.child(spacer());
    if let Some(rate) = speed {
        row = row.child(label(rate).class(classes!("text-xs text-gray-11")));
    }
    row = row.child(meter_button(
        app.clone(),
        model,
        Target::Context,
        meter("ctx", chrome::context_percent(model), "bg-accent"),
        width,
    ));
    row.child(meter_button(
        app,
        model,
        Target::Cache,
        meter("cache", chrome::cache_percent(model), "bg-info-11"),
        width,
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

fn showing(app: &App, target: &Target) -> bool {
    let pinned = match target {
        Target::Context => app.context_open.peek(),
        Target::Cache => app.cache_open.peek(),
    };
    app.meter_hover
        .peek()
        .map_or(pinned, |hovered| hovered == *target)
}

fn meter_button(
    app: App,
    model: &Model,
    target: Target,
    child: NodeBuilder,
    width: usize,
) -> NodeBuilder {
    let press = app.clone();
    let enter = app.clone();
    let leave = app.clone();
    let down_target = target.clone();
    let enter_target = target.clone();
    let mut button = container()
        .layout(
            LayoutStyle::flex_row()
                .align_items(AlignItems::Center)
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .position(Position::Relative),
        )
        .child(child);
    if showing(&app, &target) {
        button = button.child(
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset_bottom(LengthPercentageAuto::Percent(1.0))
                        .inset_right(0.0),
                )
                .child(meter_card(model, width, target.clone())),
        );
    }
    button
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

fn meter_card(model: &Model, width: usize, target: Target) -> NodeBuilder {
    let (title, rows) = match target {
        Target::Context => (" ctx ", chrome::context_facts(model)),
        Target::Cache => (" cache ", chrome::cache_facts(model)),
    };
    let card_width = width.min(30);
    let mut card = container()
        .layout(LayoutStyle::flex_column().width(card_width as f32 * canvas::cell().w))
        .class(classes!("bg-gray-1"));
    card = card.child(
        label(format!(
            "╭{title}{}╮",
            "─".repeat(card_width.saturating_sub(cell_width(title) + 2))
        ))
        .class(classes!("text-xs font-mono text-gray-11")),
    );
    for (key, value) in rows {
        card = card.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .align_items(AlignItems::Center)
                        .gap(0.0),
                )
                .child(label(format!("│ {key}")).class(classes!("text-xs font-mono text-gray-11")))
                .child(spacer())
                .child(
                    label(format!("{value} │")).class(classes!("text-xs font-mono text-gray-12")),
                ),
        );
    }
    card.child(
        label(format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))))
            .class(classes!("text-xs font-mono text-gray-11")),
    )
}
