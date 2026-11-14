use sigil::prelude::{
    AlignItems, Length, MouseButton, NodeBuilder, Paint, Position, Space, Widget, container, spacer,
};

use super::super::{App, canvas, cell_width};
use super::{BAR_CELLS, Target};
use crate::text_style::TextStyle;
use sigil::prelude::Token;

#[expect(clippy::cast_precision_loss, reason = "a percentage fits in f32")]
pub(super) fn meter(caption: &str, percent: u64, tone: sigil::prelude::Tone) -> NodeBuilder {
    let bar = f32::from(BAR_CELLS) * sigil::Host::CELL;
    sigil::prelude::row()
        .align_items(AlignItems::Center)
        .gap(Space::Sm)
        .child(TextStyle::xs(Token::Gray11).line(caption))
        .child(container().width(bar).shrink(0.0).child(
            sigil::prelude::progress(sigil::prelude::signal(percent as f32), 100.0).tone(tone),
        ))
        .child(TextStyle::xs(Token::Gray12).line(format!("{percent}%")))
}

pub(super) fn meter_button(
    app: App,
    target: Target,
    child: NodeBuilder,
    card: Option<Vec<(String, String)>>,
    width: usize,
) -> NodeBuilder {
    let mut button = sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(AlignItems::Center)
        .shrink(0.0)
        .min_width(0.0)
        .min_height(0.0)
        .position(Position::Relative)
        .child(child);
    if let Some(rows) = card {
        button = button.child(
            container()
                .position(Position::Absolute)
                .inset_bottom(Length::Percent(1.0))
                .inset_right(0.0)
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
    let mut card = sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .width(card_width as f32 * canvas::cell().w)
        .background(sigil::prelude::Token::Gray1);
    let border = TextStyle::xs(Token::Gray11).mono();
    card = card.child(border.line(format!(
        "╭{title}{}╮",
        "─".repeat(card_width.saturating_sub(cell_width(title) + 2))
    )));
    for (key, value) in rows {
        card = card.child(
            sigil::prelude::row()
                .fill_width()
                .align_items(AlignItems::Center)
                .gap(Space::None)
                .child(border.line(format!("│ {key}")))
                .child(spacer())
                .child(
                    TextStyle::xs(Token::Gray12)
                        .mono()
                        .line(format!("{value} │")),
                ),
        );
    }
    card.child(border.line(format!("╰{}╯", "─".repeat(card_width.saturating_sub(2)))))
}
