use super::*;
use sigil::prelude::Widget;
use sigil::prelude::{Dimension, MouseButton, Position, Radius};

use crate::tui::sigil_view::chat::Target;


pub(in crate::tui::sigil_view) const CONTEXT_USED: &str = "Context used";
const CARD: f32 = 240.0;
const HIGH: u64 = 80;

/// Spend and the latest notice under the composer.
pub(in crate::tui::sigil_view) fn meters(app: App) -> NodeBuilder {
    let spend = app.model.get().spend();
    let mut facts = container().layout(
        LayoutStyle::flex_row()
            .gap(L)
            .padding_top(S)
            .padding_left(L)
            .min_size(0.0, 0.0)
            .align_items(AlignItems::Center),
    );
    if let Some(spend) = spend {
        facts = facts.child(quiet(crate::provider::dollars(spend)));
    }
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(XL + XS)
                .gap(L)
                .align_items(AlignItems::Center),
        )
        .child(facts)
        .child(spacer());
    if let Some(note) = app.notice.get() {
        let tone = match note.kind {
            crate::tui::sigil_view::app::NoticeKind::Copied => super::super::style::POSITIVE,
            crate::tui::sigil_view::app::NoticeKind::Failed => super::super::style::DANGER,
        };
        row = row.child(quiet(note.text).text_color(tone));
    }
    row
}

/// The context ring in the composer: how full the context window is, with its facts on click.
#[expect(clippy::cast_precision_loss, reason = "a percentage fits in f32")]
pub(in crate::tui::sigil_view) fn ring(app: &App) -> Option<NodeBuilder> {
    let (percent, card) = {
        let model = app.model.get();
        let started = !model.usage.is_empty() || model.entries.asked();
        let percent = started.then(|| model.context_percent()).flatten()?;
        let card = app.context_open.get().then(|| model.context_facts());
        (percent, card)
    };
    let fill = if percent >= HIGH { Token::Warn9 } else { Token::Accent9 };
    let share = percent.min(100) as f32 / 100.0;
    let toggle = app.clone();
    let mut node = container()
        .layout(
            LayoutStyle::flex_row()
                .width(RING + 2.0 * XS)
                .height(super::super::style::LINE + XS)
                .justify_content(sigil::prelude::JustifyContent::Center)
                .align_items(AlignItems::Center)
                .position(Position::Relative)
                .shrink(0.0),
        )
        .hittable(true)
        .label(format!("{CONTEXT_USED} {percent}%"))
        .child(arc(1.0, Token::BorderSubtle))
        .child(
            container()
                .layout(LayoutStyle::block().position(Position::Absolute).inset_left(XS).inset_top((super::super::style::LINE + XS - RING) / 2.0))
                .child(arc(share, fill)),
        )
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Left) {
                toggle.toggle_meter(Target::Context);
                event.stop_propagation();
            }
        });
    if let Some(rows) = card {
        node = node.child(facts_card(rows, true));
    }
    Some(node)
}

const RING: f32 = 16.0;
const RING_PX: u32 = 16;
const RING_WIDTH: f32 = 2.5;
const RING_STEPS: usize = 48;

fn arc(share: f32, color: Token) -> NodeBuilder {
    let radius = (RING - RING_WIDTH) / 2.0;
    let centre = RING / 2.0;
    let steps = ((RING_STEPS as f32 * share).ceil() as usize).max(1);
    let sweep = std::f32::consts::TAU * share;
    let point = |step: usize| {
        let angle = -std::f32::consts::FRAC_PI_2 + sweep * step as f32 / steps as f32;
        (centre + radius * angle.cos(), centre + radius * angle.sin())
    };
    let (x, y) = point(0);
    let mut pen = sigil::widgets::path(RING_PX, RING_PX).move_to(x, y);
    for step in 1..=steps {
        let (x, y) = point(step);
        pen = pen.line_to(x, y);
    }
    pen.stroke(color, RING_WIDTH).fit().into()
}

fn facts_card(rows: Vec<(String, String)>, right: bool) -> NodeBuilder {
    let mut card = container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset_bottom(Dimension::Percent(1.0))
                .inset_left(if right { Dimension::Auto } else { Dimension::Px(0.0) })
                .inset_right(if right { Dimension::Px(0.0) } else { Dimension::Auto })
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
                .layout(LayoutStyle::flex_row().fill_width().align_items(AlignItems::Center))
                .child(quiet(key))
                .child(spacer())
                .child(super::super::style::text(Text::Meta, value)),
        );
    }
    card
}
