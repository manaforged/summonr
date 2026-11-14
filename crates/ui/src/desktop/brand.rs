use sigil::animation::{Easing, clock, reduced_motion};
use sigil::geometry::{Point, Rect};
use sigil::prelude::{
    NodeBuilder, Paint, Radius, Signal, Widget, container, derived, effect, signal,
};
use sigil::reactive::defer;
use sigil::scene::{DrawRect, SceneBuilder};
use sigil::style::Color;

use crate::canvas::rgb;

const ENTRANCE: f64 = 0.45;
const SCALE: f32 = 0.96;
const IRON: Color = rgb(28, 29, 26);
const FILL: f32 = 0.82;
const SMALL: f32 = 32.0;
const SNAP: f32 = 2.0;

/// The summonr.dev badge at `size` points, settling in once unless motion is reduced.
pub(super) fn mark(size: f32) -> NodeBuilder {
    let settled = signal(reduced_motion());
    container()
        .width(size)
        .height(size)
        .shrink(0.0)
        .children_reactive(move || {
            if settled.get() {
                return vec![badge(size)];
            }
            vec![entrance(settled, size)]
        })
}

fn entrance(settled: Signal<bool>, size: f32) -> NodeBuilder {
    let elapsed = clock();
    let progress =
        derived(move || Easing::CubicOut.apply((elapsed.get() / ENTRANCE).min(1.0) as f32));
    let scale = derived(move || SCALE + (1.0 - SCALE) * progress.get());
    effect(move || {
        if elapsed.get() >= ENTRANCE {
            defer(move || settled.set(true));
        }
    });
    badge(size)
        .drive(sigil::animation::Property::Opacity, progress)
        .drive(sigil::animation::Property::ScaleX, scale)
        .drive(sigil::animation::Property::ScaleY, scale)
}

/// The summonr.dev badge: an iron tile with the pixel Will on it.
pub(super) fn badge(size: f32) -> NodeBuilder {
    container()
        .width(size)
        .height(size)
        .shrink(0.0)
        .background(IRON)
        .radius(if size < SMALL { Radius::Sm } else { Radius::Lg })
        .border_width(1)
        .border_color(super::style::OUTLINE)
        .child(sigil::widgets::canvas(draw).fill())
}

fn draw(scene: &mut SceneBuilder, area: Rect) {
    use crate::scene::sprite::{Pixel, Size, frame};
    let size = if area.width < SMALL { Size::Compact } else { Size::Banner };
    let pixels = frame(size, 0, true);
    let rows = pixels.len() as f32;
    let cols = pixels.first().map_or(1, Vec::len) as f32;
    let unit = ((area.width * FILL / cols).min(area.height * FILL / rows) * SNAP).floor().max(1.0) / SNAP;
    let left = area.origin.x + (area.width - cols * unit) / 2.0;
    let top = area.origin.y + (area.height - rows * unit) / 2.0;
    let ink = crate::will::will_ink(summonr_agent::prefs::Tint::Auto, false);
    for (y, line) in pixels.iter().enumerate() {
        for (x, pixel) in line.iter().enumerate() {
            let colour = match pixel {
                Pixel::Empty => continue,
                Pixel::Edge => ink.edge,
                Pixel::Mid => ink.mid,
                Pixel::Core => ink.core,
                Pixel::Eye => ink.eye,
            };
            let at = Point::new(left + x as f32 * unit, top + y as f32 * unit);
            scene.draw_rect(DrawRect::new(Rect::new(at, unit, unit), colour));
        }
    }
}
