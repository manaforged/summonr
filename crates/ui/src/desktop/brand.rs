use sigil::animation::{Easing, clock, reduced_motion};
use sigil::geometry::{Point, Rect};
use sigil::prelude::{
    LayoutStyle, NodeBuilder, Paint, Radius, Signal, Widget, container, derived, effect, signal,
};
use sigil::reactive::defer;
use sigil::scene::{DrawRect, SceneBuilder};
use sigil::style::Rgba8;

use crate::canvas::rgb;

const ENTRANCE: f64 = 0.45;
const SCALE: f32 = 0.96;
const IRON: Rgba8 = rgb(28, 29, 26);
const WIRE: Rgba8 = rgb(86, 86, 80);
const BONE: Rgba8 = rgb(236, 231, 220);
const VIEW: (f32, f32, f32) = (48.0, 3.0, 412.0);
const GLYPH: [(f32, f32); 12] = [
    (92.0, 124.0),
    (344.0, 124.0),
    (344.0, 172.0),
    (140.0, 172.0),
    (140.0, 212.0),
    (344.0, 212.0),
    (344.0, 348.0),
    (92.0, 348.0),
    (92.0, 300.0),
    (296.0, 300.0),
    (296.0, 260.0),
    (92.0, 260.0),
];
const LAYERS: u8 = 18;
const DETAIL: f32 = 40.0;
const FACE: f32 = 12.0;
const FACE_MIN: f32 = 1.5;

/// The summonr.dev badge at `size` points, settling in once unless motion is reduced.
pub(super) fn mark(size: f32) -> NodeBuilder {
    let settled = signal(reduced_motion());
    container()
        .layout(LayoutStyle::block().width(size).height(size).shrink(0.0))
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
        .animate_opacity(progress)
        .animate_scale_x(scale)
        .animate_scale_y(scale)
}

/// The summonr.dev badge: an iron tile with layered wire behind the bone mark.
pub(super) fn badge(size: f32) -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().width(size).height(size).shrink(0.0))
        .bg(IRON)
        .rounded(if size < 32.0 { Radius::Sm } else { Radius::Lg })
        .border_w(1)
        .border_color(super::style::OUTLINE)
        .child(sigil::widgets::canvas(draw).layout(LayoutStyle::block().fill()))
}

fn draw(scene: &mut SceneBuilder, area: Rect) {
    let unit = area.width / VIEW.2;
    let origin = Point::new(area.origin.x - VIEW.0 * unit, area.origin.y - VIEW.1 * unit);
    let layers = if area.width < DETAIL { 0 } else { LAYERS };
    for layer in (1..=layers).rev() {
        let shift = (f32::from(layer) * 4.0, f32::from(layer) * -3.0);
        outline(scene, origin, unit, shift, (6.0, WIRE));
    }
    outline(
        scene,
        origin,
        unit,
        (0.0, 0.0),
        (FACE.max(FACE_MIN / unit), BONE),
    );
}

fn outline(
    scene: &mut SceneBuilder,
    origin: Point,
    unit: f32,
    shift: (f32, f32),
    (width, color): (f32, Rgba8),
) {
    for (at, from) in GLYPH.iter().enumerate() {
        let to = GLYPH[(at + 1) % GLYPH.len()];
        let half = width / 2.0;
        let left = from.0.min(to.0) - half + shift.0;
        let top = from.1.min(to.1) - half + shift.1;
        let rect = Rect::new(
            Point::new(origin.x + left * unit, origin.y + top * unit),
            ((from.0 - to.0).abs() + width) * unit,
            ((from.1 - to.1).abs() + width) * unit,
        );
        scene.draw_rect(DrawRect::new(rect, color));
    }
}
