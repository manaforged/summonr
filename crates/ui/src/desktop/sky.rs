use sigil::prelude::{LayoutStyle, NodeBuilder, Position, Signal, Widget, container, signal};
use sigil::scene::Wash;
use sigil::style::Rgba8;

use super::App;
use crate::canvas::rgb;
use crate::scene::DAY;
use crate::theme::Palette;

const NIGHT: [Rgba8; 4] = [
    rgb(36, 92, 178),
    rgb(78, 62, 176),
    rgb(136, 52, 160),
    rgb(186, 54, 118),
];

/// The watercolour sky: a pigment band on paper by day, a starred galaxy by night.
pub(super) fn layer(appearance: Signal<Palette>, seed: u64) -> NodeBuilder {
    paint(appearance, signal(seed), signal(true))
}

/// The sky behind the main column, painted while `shown` is true.
pub(super) fn behind(app: &App, shown: Signal<bool>) -> NodeBuilder {
    paint(app.appearance, app.sky, shown)
}

fn paint(appearance: Signal<Palette>, seed: Signal<u64>, shown: Signal<bool>) -> NodeBuilder {
    let version = sigil::reactive::derived(move || {
        let base = appearance.get().base;
        let paper = u64::from(u32::from_be_bytes([base.r, base.g, base.b, base.a]));
        (paper << 1 | u64::from(shown.get())) ^ seed.get().rotate_left(33)
    });
    container().layout(fill()).child(
        sigil::widgets::canvas(move |scene, area| {
            if !shown.peek() {
                return;
            }
            let paper = Rgba8 {
                a: 255,
                ..appearance.peek().base
            };
            scene.draw_wash(area, wash(paper, seed.peek()));
        })
        .redraw_on(version)
        .layout(LayoutStyle::block().fill()),
    )
}

fn fill() -> LayoutStyle {
    LayoutStyle::block()
        .position(Position::Absolute)
        .inset_top(0.0)
        .inset_left(0.0)
        .inset_right(0.0)
        .inset_bottom(0.0)
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
