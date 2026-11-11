use sigil::prelude::{
    AlignItems, ControlSize, FlexWrap, JustifyContent, NodeBuilder, Paint, Position, Radius,
    Signal, Widget, button, signal,
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
const TRIES: [(&str, Mood); 11] = [
    ("Idle", Mood::Idle),
    ("Summon", Mood::Summoned),
    ("Notice", Mood::Notice),
    ("Typing", Mood::Typing),
    ("Thinking", Mood::Thinking),
    ("Working", Mood::Working),
    ("Needs you", Mood::Needs),
    ("Done", Mood::Passed),
    ("Failed", Mood::Failed),
    ("Stopped short", Mood::Stopped),
    ("Doze", Mood::Doze),
];

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    let mood = signal(Mood::Idle);
    vec![stage(app, mood), tries(mood), options(app), reactions(app)]
}

fn stage(app: &App, mood: Signal<Mood>) -> NodeBuilder {
    sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .fill_width()
        .height(STAGE)
        .position(Position::Relative)
        .align_items(AlignItems::Center)
        .justify_content(JustifyContent::Center)
        .overflow(sigil::prelude::Overflow::Hidden)
        .radius(Radius::Lg)
        .child(super::super::sky::layer(app.appearance, app.sky.get()))
        .child(super::super::will::preview(app, SHOWN, mood))
}

fn tries(mood: Signal<Mood>) -> NodeBuilder {
    sigil::prelude::row()
        .gap(S)
        .flex_wrap(FlexWrap::Wrap)
        .align_items(AlignItems::Center)
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
        Reaction::Look => (
            "Follow the pointer",
            "His eyes follow your pointer around the window",
        ),
        Reaction::Type => ("Read along", "Lean toward the text while you type"),
        Reaction::Send => ("Hop", "Hop when you send, then think and work"),
        Reaction::Finish => (
            "Brighten",
            "Brighten with a small spark burst when work finishes well",
        ),
        Reaction::Fail => ("Flinch", "Flinch, then squash and dim when work fails"),
        Reaction::Stop => ("Tilt", "Tilt, unsure, when you stop the agent"),
        Reaction::Ask => (
            "Glance",
            "Glance aside with an amber dot while the agent needs you",
        ),
        Reaction::Hover => (
            "Notice",
            "Widen his eyes and lift a little when you hover him",
        ),
        Reaction::Poke => ("Poke", "Hop with a spark burst when you click him"),
        Reaction::Doze => ("Doze", "Close his eyes after 12 seconds alone"),
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
