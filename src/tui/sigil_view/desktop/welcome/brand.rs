use sigil::animation::{Easing, clock, reduced_motion};
use sigil::prelude::{LayoutStyle, NodeBuilder, Widget, container, derived, effect, signal};
use sigil::reactive::defer;
use sigil::widgets::icon_svg;

const MARK: &str = include_str!("../../../../../assets/mark.svg");
const SIZE: f32 = 96.0;
const ENTRANCE: f64 = 0.45;
const SCALE: f32 = 0.96;

/// Render the website-derived mark with a finite, reduced-motion-aware entrance.
pub(super) fn mark() -> NodeBuilder {
    let settled = signal(reduced_motion());
    container()
        .layout(LayoutStyle::block().width(SIZE).height(SIZE).shrink(0.0))
        .children_reactive(move || {
            if settled.get() {
                return vec![icon_svg(MARK).px(SIZE).label("Summonr").into()];
            }
            vec![entrance(settled)]
        })
}

fn entrance(settled: sigil::prelude::Signal<bool>) -> NodeBuilder {
    let elapsed = clock();
    let progress =
        derived(move || Easing::CubicOut.apply((elapsed.get() / ENTRANCE).min(1.0) as f32));
    let scale = derived(move || SCALE + (1.0 - SCALE) * progress.get());
    effect(move || {
        if elapsed.get() >= ENTRANCE {
            defer(move || settled.set(true));
        }
    });
    icon_svg(MARK)
        .px(SIZE)
        .label("Summonr")
        .animate_opacity(progress)
        .animate_scale_x(scale)
        .animate_scale_y(scale)
        .into()
}
