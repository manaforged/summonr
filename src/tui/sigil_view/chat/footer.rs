use sigil::layout::LengthPercentageAuto;
use sigil::layout::Position;
use sigil::prelude::AlignItems;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::MouseButton;
use sigil::prelude::NodeBuilder;
use sigil::prelude::NodeEventsExt;
use sigil::prelude::Paint;
use sigil::prelude::container;
use sigil::prelude::spacer;

use super::{App, Entry, canvas, cell_width};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Target {
    Context,
    Cache,
}

type FooterView = (
    Option<super::super::app::Notice>,
    Option<String>,
    Option<(String, bool)>,
    Option<u64>,
    Option<u64>,
    Option<(bool, Vec<(String, String)>)>,
);

fn footer_view(app: &App) -> FooterView {
    drop(app.status.get());
    let context_open = app.context_open.get();
    let cache_open = app.cache_open.get();
    let model = app.model.get();
    let spend = model.spend().map(crate::dollars);
    let started = !model.usage.is_empty()
        || model
            .entries
            .iter()
            .any(|entry| matches!(entry, Entry::User(_)));
    let open = if !started {
        None
    } else if context_open {
        Some((false, model.context_facts()))
    } else if cache_open {
        Some((true, model.cache_facts()))
    } else {
        None
    };
    (
        app.notice.get(),
        spend,
        elsewhere(&model.live),
        started.then(|| model.context_percent()).flatten(),
        started.then(|| model.cache_percent()),
        open,
    )
}

fn elsewhere(live: &[crate::tui::hub::Live]) -> Option<(String, bool)> {
    let others: Vec<_> = live.iter().filter(|entry| !entry.active).collect();
    if others.is_empty() {
        return None;
    }
    let waiting = others
        .iter()
        .filter(|entry| entry.status == crate::tui::hub::Status::NeedsYou)
        .count();
    let open = crate::text::plural(others.len(), "other session");
    Some(if waiting == 0 {
        (open, false)
    } else {
        (format!("{open} · {waiting} waiting on you · /resume"), true)
    })
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
    let (notice, spend, elsewhere, context_pct, cache_pct, open) = view;
    let taken = cache_pct.map_or(0, |_| 42)
        + elsewhere
            .as_ref()
            .map_or(0, |(note, _)| cell_width(note) + 2)
        + app
            .git_branch
            .as_deref()
            .map_or(0, |branch| cell_width(branch) + 4)
        + spend.as_deref().map_or(0, |cost| cell_width(cost) + 2)
        + notice.as_ref().map_or(0, |note| cell_width(&note.text) + 2);
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
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray12),
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
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(12.0)
                        .text_color(sigil::prelude::Token::Gray11),
                )
                .child(
                    sigil::kernel::text(branch.clone())
                        .nowrap()
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(12.0)
                        .text_color(sigil::prelude::Token::Warn11),
                ),
        );
    }
    row = row.child(spacer());
    if let Some(note) = notice {
        let tone = match note.kind {
            super::super::app::NoticeKind::Copied => sigil::prelude::Token::Success11,
            super::super::app::NoticeKind::Failed => sigil::prelude::Token::Error11,
        };
        row = row.child(
            sigil::kernel::text(note.text)
                .nowrap()
                .font_size(12.0)
                .text_color(tone),
        );
    }
    if let Some((note, waiting)) = elsewhere {
        row = row.child(
            sigil::kernel::text(note)
                .nowrap()
                .font_size(12.0)
                .text_color(if waiting {
                    sigil::prelude::Token::Warn11
                } else {
                    sigil::prelude::Token::Gray11
                }),
        );
    }
    if let Some(cost) = spend {
        row = row.child(
            sigil::kernel::text(cost)
                .nowrap()
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray12),
        );
    }
    let meters = [
        (
            false,
            Target::Context,
            "ctx",
            context_pct,
            sigil::prelude::Token::Accent,
        ),
        (
            true,
            Target::Cache,
            "cache",
            cache_pct,
            sigil::prelude::Token::Info11,
        ),
    ];
    for (cache, target, caption, percent, fill) in meters {
        let Some(percent) = percent else {
            continue;
        };
        let card = open
            .as_ref()
            .and_then(|(shown, rows)| (*shown == cache).then(|| rows.clone()));
        row = row.child(meter_button(
            app.clone(),
            target,
            meter(caption, percent, fill),
            card,
            width,
        ));
    }
    row
}

fn meter(caption: &str, percent: u64, fill: sigil::prelude::Token) -> NodeBuilder {
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
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_row().width(bar).height(8.0))
                .bg(sigil::prelude::Token::Gray1)
                .child(
                    container()
                        .layout(
                            LayoutStyle::block()
                                .width(bar * percent.min(100) as f32 / 100.0)
                                .height(8.0),
                        )
                        .bg(fill),
                ),
        )
        .child(
            sigil::kernel::text(format!("{percent}%"))
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray12),
        )
}

fn meter_button(
    app: App,
    target: Target,
    child: NodeBuilder,
    card: Option<Vec<(String, String)>>,
    width: usize,
) -> NodeBuilder {
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
                .pointer_down(|event| event.stop_propagation())
                .child(meter_card(rows, width, target.clone())),
        );
    }
    button.pointer_down(move |event| {
        if event.button == Some(MouseButton::Left) {
            app.toggle_meter(target.clone());
            event.stop_propagation();
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
        .bg(sigil::prelude::Token::Gray1);
    card = card.child(
        sigil::kernel::text(format!(
            "╭{title}{}╮",
            "─".repeat(card_width.saturating_sub(cell_width(title) + 2))
        ))
        .nowrap()
        .text_color(sigil::prelude::Token::OnSurface)
        .font_size(12.0)
        .font_family(sigil::prelude::FontFamily::Mono)
        .text_color(sigil::prelude::Token::Gray11),
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
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(12.0)
                        .font_family(sigil::prelude::FontFamily::Mono)
                        .text_color(sigil::prelude::Token::Gray11),
                )
                .child(spacer())
                .child(
                    sigil::kernel::text(format!("{value} │"))
                        .nowrap()
                        .text_color(sigil::prelude::Token::OnSurface)
                        .font_size(12.0)
                        .font_family(sigil::prelude::FontFamily::Mono)
                        .text_color(sigil::prelude::Token::Gray12),
                ),
        );
    }
    card.child(
        sigil::kernel::text(format!("╰{}╯", "─".repeat(card_width.saturating_sub(2))))
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_size(12.0)
            .font_family(sigil::prelude::FontFamily::Mono)
            .text_color(sigil::prelude::Token::Gray11),
    )
}
