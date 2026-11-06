use sigil::prelude::{LayoutStyle, NodeBuilder, Position, Signal, Widget, container};
use sigil::scene::Wash;
use sigil::style::Rgba8;

use super::App;
use crate::canvas::rgb;
use crate::theme::Palette;

const DAY: [Rgba8; 4] = [
    rgb(64, 132, 214),
    rgb(92, 96, 206),
    rgb(168, 82, 190),
    rgb(226, 88, 138),
];
const NIGHT: [Rgba8; 4] = [
    rgb(36, 92, 178),
    rgb(78, 62, 176),
    rgb(136, 52, 160),
    rgb(186, 54, 118),
];

/// The watercolour sky: a pigment band on paper by day, a starred galaxy by night.
pub(super) fn layer(appearance: Signal<Palette>, seed: u64) -> NodeBuilder {
    let version = sigil::reactive::derived(move || {
        let base = appearance.get().base;
        u64::from(u32::from_be_bytes([base.r, base.g, base.b, base.a]))
    });
    container().layout(fill()).child(
        sigil::widgets::canvas(move |scene, area| {
            let paper = Rgba8 {
                a: 255,
                ..appearance.peek().base
            };
            scene.draw_wash(area, wash(paper, seed));
        })
        .redraw_on(version)
        .layout(LayoutStyle::block().fill()),
    )
}

/// An empty layer behind the main column that holds the sky while `shown` is true.
pub(super) fn behind(app: &App, shown: Signal<bool>) -> NodeBuilder {
    let (appearance, seed) = (app.appearance, app.sky);
    container().layout(fill()).children_reactive(move || {
        if shown.get() {
            vec![layer(appearance, seed.get())]
        } else {
            Vec::new()
        }
    })
}

fn fill() -> LayoutStyle {
    LayoutStyle::block()
        .position(Position::Absolute)
        .inset_top(0.0)
        .inset_left(0.0)
        .fill()
}

fn wash(paper: Rgba8, seed: u64) -> Wash {
    let light = u32::from(paper.r) + u32::from(paper.g) + u32::from(paper.b) > 3 * 128;
    let (pigments, specks) = if light { (DAY, 0.35) } else { (NIGHT, 0.7) };
    let (pools, edges, density) = if light {
        (4, 0.55, 1.5)
    } else {
        (0, 0.25, 1.0)
    };
    pigments
        .into_iter()
        .fold(Wash::new(paper), Wash::pigment)
        .seed(seed as u32)
        .band(true)
        .density(density)
        .pools(pools)
        .edges(edges)
        .granulation(0.55)
        .grain(if light { 0.6 } else { 0.15 })
        .specks(specks)
}
