use super::space::{FLARE, GLASS, PLATE, SCRIM, STAR_BRIGHT, STAR_DIM, VOID, WARM};
use super::stars::{DENSITY, TINTS};
use super::*;

const DAY_PANE: f32 = 0.92;

const DAY_LABEL: f32 = 0.35;

const DAY_DENSITY: [f32; 2] = [0.05, 3.2];

const INKS: [Rgba8; 10] = [
    rgb(34, 66, 128),
    rgb(44, 62, 136),
    rgb(52, 50, 130),
    rgb(62, 44, 124),
    rgb(78, 40, 120),
    rgb(96, 36, 112),
    rgb(112, 34, 100),
    rgb(124, 34, 88),
    rgb(52, 50, 130),
    rgb(62, 44, 124),
];

/// The colors the start screen is drawn in: a night sky on dark themes, paper on light ones.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Stage {
    pub(crate) paper: bool,
    pub(crate) void: Rgba8,
    pub(crate) scrim: Rgba8,
    pub(crate) plate: Rgba8,
    pub(crate) glass: f32,
    pub(crate) pane: f32,
    pub(super) label: Rgba8,
    pub(super) value: Rgba8,
    pub(super) star_dim: Rgba8,
    pub(super) star_bright: Rgba8,
    pub(super) warm: Rgba8,
    pub(super) flare: Rgba8,
    pub(super) tints: [Rgba8; 10],
    pub(super) density: [f32; 2],
}

const NIGHT: Stage = Stage {
    paper: false,
    void: VOID,
    scrim: SCRIM,
    plate: PLATE,
    glass: GLASS,
    pane: GLASS,
    label: VOID,
    value: VOID,
    star_dim: STAR_DIM,
    star_bright: STAR_BRIGHT,
    warm: WARM,
    flare: FLARE,
    tints: TINTS,
    density: DENSITY,
};

/// The stage for the active theme.
pub(crate) fn stage() -> Stage {
    let active = palette();
    let base = active.base;
    let [r, g, b] = [base.r, base.g, base.b].map(u32::from);
    if r * 299 + g * 587 + b * 114 <= 128_000 {
        return Stage {
            label: active.dim,
            value: active.gray,
            ..NIGHT
        };
    }
    let paper = rgb(base.r, base.g, base.b);
    Stage {
        paper: true,
        void: paper,
        scrim: paper,
        plate: paper,
        glass: GLASS,
        pane: DAY_PANE,
        label: blend(active.text, active.dim, DAY_LABEL),
        value: active.text,
        star_dim: blend(paper, INKS[3], 0.45),
        star_bright: INKS[7],
        warm: INKS[7],
        flare: INKS[2],
        tints: INKS,
        density: DAY_DENSITY,
    }
}
