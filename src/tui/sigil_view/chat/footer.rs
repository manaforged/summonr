use sigil::layout::LengthPercentageAuto;
use sigil::layout::Position;
use sigil::prelude::AlignItems;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::NodeStyleExt;
use sigil::prelude::container;
use sigil::prelude::spacer;

use super::chrome;
use super::{App, Speed, canvas, cell_width};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Target {
    Context,
    Cache,
}

type FooterView = (
    Option<String>,
    u64,
    u64,
    Option<(bool, Vec<(String, String)>)>,
);

fn footer_view(app: &App) -> FooterView {
    drop(app.status.get());
    let context_open = app.context_open.get();
    let cache_open = app.cache_open.get();
    let hover = app.meter_hover.get();
    let model = app.model.get();
    let speed = model.speed.last().map(Speed::label);
    let open = if hover == Some(Target::Context) || (hover.is_none() && context_open) {
        Some((false, chrome::context_facts(&model)))
    } else if hover == Some(Target::Cache) || (hover.is_none() && cache_open) {
        Some((true, chrome::cache_facts(&model)))
    } else {
        None
    };
    (
        speed,
        chrome::context_percent(&model),
        chrome::cache_percent(&model),
        open,
    )
}

pub(super) fn footer_bar(app: App, width: usize) -> NodeBuilder {
    let update = app.clone();
    let view = sigil::reactive::derived(move || footer_view(&update));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || vec![footer_row(app.clone(), view.get(), width)])
}

fn footer_row(app: App, view: FooterView, width: usize) -> NodeBuilder {
    let (speed, context_pct, cache_pct, open) = view;
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
        row = row.child(
            sigil::kernel::text(place)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-12"),
        );
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
                .child(
                    sigil::kernel::text("⎇")
                        .nowrap()
                        .class(sigil::style::Token::OnSurface.text())
                        .class("text-xs text-gray-11"),
                )
                .child(
                    sigil::kernel::text(branch.clone())
                        .nowrap()
                        .class(sigil::style::Token::OnSurface.text())
                        .class("text-xs text-warn-11"),
                ),
        );
    }
    row = row.child(spacer());
    if let Some(rate) = speed {
        row = row.child(
            sigil::kernel::text(rate)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-11"),
        );
    }
    let context_card = open
        .as_ref()
        .and_then(|(cache, rows)| (!*cache).then_some(rows.clone()));
    let cache_card = open
        .as_ref()
        .and_then(|(cache, rows)| (*cache).then_some(rows.clone()));
    row = row.child(meter_button(
        app.clone(),
        Target::Context,
        meter("ctx", context_pct, "bg-accent"),
        context_card,
        width,
    ));
    row.child(meter_button(
        app,
        Target::Cache,
        meter("cache", cache_pct, "bg-info-11"),
        cache_card,
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
        .child(
            sigil::kernel::text(caption)
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-11"),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_row().width(bar).height(8.0))
                .class("bg-gray-1")
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
        .child(
            sigil::kernel::text(format!("{percent}%"))
                .nowrap()
                .class(sigil::style::Token::OnSurface.text())
                .class("text-xs text-gray-12"),
        )
}

fn meter_button(
    app: App,
    target: Target,
    child: NodeBuilder,
    card: Option<Vec<(String, String)>>,
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
    if let Some(rows) = card {
        button = button.child(
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset_bottom(LengthPercentageAuto::Percent(1.0))
                        .inset_right(0.0),
                )
                .child(meter_card(rows, width, target.clone())),
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

fn meter_card(rows: Vec<(String, String)>, width: usize, target: Target) -> NodeBuilder {
    let title = match target {
        Target::Context => " ctx ",
        Target::Cache => " cache ",
    };
    let card_width = width.min(30);
    let mut card = container()
        .layout(LayoutStyle::flex_column().width(card_width as f32 * canvas::cell().w))
        .class("bg-gray-1");
    card = card.child(
        sigil::kernel::text(format!(
            "╭{title}{}╮",
            "─".repeat(card_width.saturating_sub(cell_width(title) + 2))
        ))
        .nowrap()
        .class(sigil::style::Token::OnSurface.text())
        .class("text-xs font-mono text-gray-11"),
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
                .child(
                    sigil::kernel::text(format!("│ {key}"))
                        .nowrap()
                        .class(sigil::style::Token::OnSurface.text())
                        .class("text-xs font-mono text-gray-11"),
                )
                .child(spacer())
                .child(
                    sigil::kernel::text(format!("{value} │"))
                        .nowrap()
                        .class(sigil::style::Token::OnSurface.text())
                        .class("text-xs font-mono text-gray-12"),
                ),
        );
    }
    card.child(
        sigil::kernel::text(format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))))
            .nowrap()
            .class(sigil::style::Token::OnSurface.text())
            .class("text-xs font-mono text-gray-11"),
    )
}
