use super::space::{FLARE, GLASS, PLATE, SCRIM, STAR_BRIGHT, STAR_DIM, VOID, WARM};
use super::stars::{DENSITY, TINTS};
use super::*;

const DAY_PANE: f32 = 0.92;

const DAY_LABEL: f32 = 0.35;

const DAY_DENSITY: [f32; 2] = [0.05, 3.2];

const INKS: [Color; 10] = [
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
    pub(crate) void: Color,
    pub(crate) scrim: Color,
    pub(crate) plate: Color,
    pub(crate) glass: f32,
    pub(crate) pane: f32,
    pub(super) label: Color,
    pub(super) value: Color,
    pub(super) star_dim: Color,
    pub(super) star_bright: Color,
    pub(super) warm: Color,
    pub(super) flare: Color,
    pub(super) tints: [Color; 10],
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
    if !active.light() {
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
