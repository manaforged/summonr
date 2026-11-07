use sigil::prelude::{
    AlignItems, JustifyContent, LayoutStyle, NodeBuilder, Paint, Position, Radius, Widget, button,
    container,
};
use summonr_agent::prefs::{Prefs, Tint};

use super::super::App;
use super::parts::{choice, row, section, switch};

/// The height of the preview of Will on the sky.
const STAGE: f32 = 160.0;
/// Will's size on the preview.
const SHOWN: f32 = 72.0;

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    vec![stage(app), options(app)]
}

fn stage(app: &App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(STAGE)
                .position(Position::Relative)
                .align_items(AlignItems::Center)
                .justify_content(JustifyContent::Center)
                .overflow(sigil::prelude::Overflow::Hidden),
        )
        .rounded(Radius::Lg)
        .child(super::super::sky::layer(app.appearance, app.sky.get()))
        .child(super::super::will::still(app, SHOWN))
}

fn options(app: &App) -> NodeBuilder {
    let prefs = app.model.get().prefs.clone();
    let (show, tint, home) = (app.clone(), app.clone(), app.clone());
    let names = Tint::ALL.map(label);
    let at = Tint::ALL.iter().position(|each| *each == prefs.will_tint).unwrap_or(0);
    section(
        None,
        vec![
            switch(
                "Show Will",
                Some("A small flame that floats over the agent and works while it works"),
                prefs.will,
                move |_| show.flip(|prefs| prefs.will = !prefs.will),
            ),
            choice("Colour", None, &names, at, move |index| {
                tint.flip(|prefs| prefs.will_tint = Tint::ALL[index.min(Tint::ALL.len() - 1)]);
            }),
            row(
                "Position",
                Some("Drag Will anywhere over the agent"),
                button("Reset")
                    .activate(move || home.flip(|prefs| prefs.will_spot = Prefs::default().will_spot))
                    .into(),
            ),
        ],
    )
}

const fn label(tint: Tint) -> &'static str {
    match tint {
        Tint::Wisp => "Wisp",
        Tint::Flame => "Flame",
        Tint::Foxfire => "Foxfire",
        Tint::Arcane => "Arcane",
    }
}
