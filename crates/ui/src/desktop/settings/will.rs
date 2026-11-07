use sigil::prelude::{
    AlignItems, ControlSize, JustifyContent, LayoutStyle, NodeBuilder, Paint, Position, Radius,
    Signal, Widget, button, container, signal,
};
use summonr_agent::prefs::{Prefs, Reaction, Tint};

use super::super::App;
use super::super::style::S;
use super::super::will::Mood;
use super::parts::{row, section, select, switch};

/// The height of the preview of Will on the sky.
const STAGE: f32 = 160.0;
/// Will's size on the preview.
const SHOWN: f32 = 72.0;

/// The moods the preview can show, with their button labels.
const TRIES: [(&str, Mood); 6] = [
    ("Idle", Mood::Idle),
    ("Working", Mood::Working),
    ("Listening", Mood::Listening),
    ("Done", Mood::Pleased),
    ("Failed", Mood::Drooping),
    ("Needs you", Mood::Asking),
];

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    let mood = signal(Mood::Idle);
    vec![stage(app, mood), tries(mood), options(app), reactions(app)]
}

fn stage(app: &App, mood: Signal<Mood>) -> NodeBuilder {
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
        .child(super::super::will::preview(app, SHOWN, mood))
}

fn tries(mood: Signal<Mood>) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .gap(S)
                .align_items(AlignItems::Center),
        )
        .children_reactive(move || {
            let shown = mood.get();
            TRIES
                .iter()
                .map(|&(label, each)| {
                    let pick = button(label).size(ControlSize::Compact);
                    let pick = if each == shown { pick } else { pick.ghost() };
                    NodeBuilder::from(pick.activate(move || mood.set(each)))
                })
                .collect::<Vec<_>>()
        })
}

fn reactions(app: &App) -> NodeBuilder {
    let quiet = app.model.get().prefs.will_quiet.clone();
    let rows = Reaction::ALL
        .iter()
        .map(|&each| {
            let (title, about) = reaction_label(each);
            let flip = app.clone();
            switch(title, Some(about), !quiet.contains(&each), move |_| {
                flip.flip(|prefs| {
                    if let Some(at) = prefs.will_quiet.iter().position(|seen| *seen == each) {
                        prefs.will_quiet.remove(at);
                    } else {
                        prefs.will_quiet.push(each);
                    }
                });
            })
        })
        .collect();
    section(Some("Reactions"), rows)
}

const fn reaction_label(reaction: Reaction) -> (&'static str, &'static str) {
    match reaction {
        Reaction::Type => ("Listen", "Lean in while you type"),
        Reaction::Send => ("Hop", "Jump when you send a message"),
        Reaction::Stop => ("Flinch", "Shut his eyes when you stop the agent"),
        Reaction::Finish => ("Smile", "Brighten when a turn finishes"),
        Reaction::Fail => ("Droop", "Sag when a turn fails"),
        Reaction::Ask => ("Look up", "Tilt his head while the agent needs you"),
    }
}

fn options(app: &App) -> NodeBuilder {
    let prefs = app.model.get().prefs.clone();
    let (show, tint, home) = (app.clone(), app.clone(), app.clone());
    let at = Tint::ALL
        .iter()
        .position(|each| *each == prefs.will_tint)
        .unwrap_or(0);
    section(
        None,
        vec![
            switch(
                "Show Will",
                Some("A small flame that floats over the agent and works while it works"),
                prefs.will,
                move |_| show.flip(|prefs| prefs.will = !prefs.will),
            ),
            select(
                "Colour",
                Some("Auto is Flame on a light theme and Wisp on a dark one"),
                Tint::ALL.map(|each| label(each).to_owned()).to_vec(),
                at,
                move |index| {
                    tint.flip(|prefs| prefs.will_tint = Tint::ALL[index.min(Tint::ALL.len() - 1)]);
                },
            ),
            row(
                "Position",
                Some("Drag Will anywhere over the agent; right-click him to hide him"),
                button("Reset")
                    .activate(move || {
                        home.flip(|prefs| prefs.will_spot = Prefs::default().will_spot)
                    })
                    .into(),
            ),
        ],
    )
}

const fn label(tint: Tint) -> &'static str {
    match tint {
        Tint::Auto => "Auto",
        Tint::Wisp => "Wisp",
        Tint::Flame => "Flame",
        Tint::Foxfire => "Foxfire",
        Tint::Arcane => "Arcane",
    }
}
