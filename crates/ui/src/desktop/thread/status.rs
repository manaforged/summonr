use super::*;
use sigil::prelude::Widget;
use sigil::prelude::{Dimension, MouseButton, Position, Radius};

use crate::chat::Target;

const CONTEXT_USED: &str = "Context used";
const CACHE_HIT: &str = "Cache hit";
const BAR: f32 = 64.0;
const CARD: f32 = 240.0;
const HIGH: u64 = 80;

/// Spend, tokens, live speed, the latest notice, and the context and cache meters under the composer.
pub(crate) fn meters(app: App) -> NodeBuilder {
    let (spend, tokens, gauges) = {
        let model = app.model.get();
        let idle = model.busy_since.is_none();
        let started = !model.usage.is_empty() || model.entries.asked();
        let context = started.then(|| model.context_percent()).flatten();
        let cache = started.then(|| model.cache_percent());
        let context_card = app.context_open.get().then(|| model.context_facts());
        let cache_card = app.cache_open.get().then(|| model.cache_facts());
        (
            model.spend(),
            (model.tokens(|round| round.prompt) + model.tokens(|round| round.completion))
                * u64::from(idle),
            [
                context.map(|percent| (Target::Context, percent, context_card)),
                cache.map(|percent| (Target::Cache, percent, cache_card)),
            ],
        )
    };
    let mut facts = container().layout(
        LayoutStyle::flex_row()
            .gap(L)
            .padding_top(S)
            .padding_left(L)
            .min_size(0.0, 0.0)
            .align_items(AlignItems::Center),
    );
    if let Some(spend) = spend {
        facts = facts.child(quiet(summonr_agent::provider::dollars(spend)));
    }
    if tokens > 0 {
        facts = facts.child(quiet(format!(
            "{} tokens",
            summonr_agent::provider::compact(tokens)
        )));
    }
    let speed = app.clone();
    facts = facts.child(
        super::super::style::text(Text::Meta, move || {
            speed.ticks.with(|_| ());
            speed.model.with(|model| {
                model
                    .speed
                    .live(std::time::Instant::now())
                    .map(crate::speed::Speed::label)
                    .unwrap_or_default()
            })
        })
        .nowrap(),
    );
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(XL + XS)
                .gap(L)
                .padding_top(S)
                .padding_right(L)
                .align_items(AlignItems::Center),
        )
        .child(facts)
        .child(spacer());
    if let Some(note) = app.notice.get() {
        let tone = match note.kind {
            crate::app::NoticeKind::Copied => super::super::style::POSITIVE,
            crate::app::NoticeKind::Failed => super::super::style::DANGER,
        };
        row = row.child(quiet(note.text).text_color(tone));
    }
    for (target, percent, card) in gauges.into_iter().flatten() {
        row = row.child(gauge(&app, target, percent, card));
    }
    row
}

/// One meter: a bar and its percent, with its facts on click.
#[expect(clippy::cast_precision_loss, reason = "a percentage fits in f32")]
fn gauge(
    app: &App,
    target: Target,
    percent: u64,
    card: Option<Vec<(String, String)>>,
) -> NodeBuilder {
    let high = target == Target::Context && percent >= HIGH;
    let (name, tone) = match target {
        Target::Context if high => (CONTEXT_USED, sigil::prelude::Tone::Warn),
        Target::Context => (CONTEXT_USED, sigil::prelude::Tone::Primary),
        Target::Cache => (CACHE_HIT, sigil::prelude::Tone::Info),
    };
    let number = quiet(format!("{percent}%"));
    let number = if high {
        number.text_color(Token::Warn11)
    } else {
        number
    };
    let toggle = app.clone();
    let mut node = container()
        .layout(
            LayoutStyle::flex_row()
                .gap(S)
                .align_items(AlignItems::Center)
                .position(Position::Relative)
                .shrink(0.0),
        )
        .hittable(true)
        .label(format!("{name} {percent}%"))
        .child(
            container()
                .layout(LayoutStyle::block().width(BAR).shrink(0.0))
                .child(
                    sigil::prelude::progress(sigil::prelude::signal(percent as f32), 100.0)
                        .tone(tone),
                ),
        )
        .child(number)
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Left) {
                toggle.toggle_meter(target.clone());
                event.stop_propagation();
            }
        });
    if let Some(rows) = card {
        node = node.child(facts_card(rows, true));
    }
    sigil::widgets::tooltip(name).child(node).into()
}

fn facts_card(rows: Vec<(String, String)>, right: bool) -> NodeBuilder {
    let mut card = container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset_bottom(Dimension::Percent(1.0))
                .inset_left(if right {
                    Dimension::Auto
                } else {
                    Dimension::Px(0.0)
                })
                .inset_right(if right {
                    Dimension::Px(0.0)
                } else {
                    Dimension::Auto
                })
                .width(CARD)
                .padding(M)
                .gap(XS),
        )
        .rounded(Radius::Lg)
        .bg(super::super::style::RAISED)
        .border_w(1)
        .border_color(super::super::style::OUTLINE)
        .pointer_down(|event| event.stop_propagation());
    for (key, value) in rows {
        card = card.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .align_items(AlignItems::Center),
                )
                .child(quiet(key))
                .child(spacer())
                .child(super::super::style::text(Text::Meta, value)),
        );
    }
    card
}
