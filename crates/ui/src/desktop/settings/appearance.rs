use sigil::prelude::{AlignItems, NodeBuilder, Paint, Radius, Widget, card, container};

use super::super::App;
use super::super::style::{M, S, Text, XS, text};
use super::parts::{choice, section, slider, switch};
use crate::theme::Palette;
use summonr_agent::prefs::{AUTO_THEME, DiffLayout, SPACING, THEMES};

pub(super) fn view(app: &App) -> Vec<NodeBuilder> {
    vec![themes(app), display(app)]
}

fn themes(app: &App) -> NodeBuilder {
    let current = app.model.get().prefs.theme.clone();
    let grid = THEMES.iter().fold(
        sigil::prelude::row()
            .align_items(sigil::prelude::AlignItems::Stretch)
            .fill_width()
            .gap(M)
            .flex_wrap(sigil::prelude::FlexWrap::Wrap),
        |grid, name| grid.child(swatch(app, name, *name == current)),
    );
    sigil::prelude::column()
        .fill_width()
        .align_items(AlignItems::Stretch)
        .gap(S)
        .child(text(Text::Heading, "Theme"))
        .child(grid)
}

fn swatch(app: &App, name: &'static str, selected: bool) -> NodeBuilder {
    let palette = Palette::named(name);
    let pick = app.clone();
    let preview = sigil::prelude::column()
        .fill_width()
        .height(56.0)
        .padding(M)
        .gap(XS)
        .radius(Radius::Md)
        .background(palette.base)
        .child(bar(palette.text, 64.0))
        .child(bar(palette.gray, 40.0))
        .child(
            sigil::prelude::row()
                .align_items(sigil::prelude::AlignItems::Stretch)
                .gap(XS)
                .child(bar(palette.green, 16.0))
                .child(bar(palette.cyan, 16.0))
                .child(bar(palette.yellow, 16.0)),
        );
    let label = label_for(name);
    let body = sigil::prelude::column()
        .fill_width()
        .gap(S)
        .align_items(AlignItems::Stretch)
        .child(preview)
        .child(text(Text::Label, label));
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .width(SWATCH)
        .shrink(0.0)
        .child(NodeBuilder::from(
            card(body)
                .selected(selected)
                .activate(move || pick.select_theme(name)),
        ))
}

fn bar(color: sigil::style::Color, width: f32) -> NodeBuilder {
    container()
        .width(width)
        .height(6.0)
        .radius(Radius::Full)
        .background(color)
}

const SWATCH: f32 = 156.0;

fn label_for(name: &str) -> String {
    if name == AUTO_THEME {
        return "Match system".to_owned();
    }
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

fn display(app: &App) -> NodeBuilder {
    let prefs = app.model.get().prefs.clone();
    let (motion, splash, diff, gap) = (app.clone(), app.clone(), app.clone(), app.clone());
    let at = usize::from(prefs.diff_layout == DiffLayout::Unified);
    section(
        Some("Display"),
        vec![
            switch(
                "Motion",
                Some("Animate spinners, artwork and transitions"),
                prefs.animations,
                move |_| {
                    motion.toggle_motion();
                },
            ),
            switch(
                "New agent artwork",
                Some("Show artwork behind an empty agent"),
                prefs.splash,
                move |_| {
                    splash.flip(|prefs| prefs.splash = !prefs.splash);
                },
            ),
            slider(
                "Paragraph spacing",
                Some("Space between paragraphs and blocks in a reply"),
                SPACING,
                prefs.spacing,
                move |px| gap.flip(|prefs| prefs.spacing = px),
            ),
            choice(
                "Changes view",
                Some("How file changes are laid out"),
                &["Side by side", "Inline"],
                at,
                move |index| {
                    diff.set_diff_layout(if index == 0 {
                        DiffLayout::Split
                    } else {
                        DiffLayout::Unified
                    });
                },
            ),
        ],
    )
}
