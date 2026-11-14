//! Will's paints, shared by the desktop rig and the terminal splash.

use sigil::prelude::Color;
use summonr_agent::prefs::Tint;

use crate::canvas::rgb;

/// Will's paints for one tint on one theme.
#[derive(Clone, Copy)]
#[cfg_attr(not(feature = "gui"), expect(dead_code))]
pub(crate) struct Ink {
    pub(crate) edge: Color,
    pub(crate) mid: Color,
    pub(crate) core: Color,
    pub(crate) eye: Color,
    pub(crate) glow: Color,
    pub(crate) warn: Color,
    pub(crate) bad: Color,
}

/// Will's paints for `tint` on a light or dark theme.
pub(crate) const fn will_ink(tint: Tint, light: bool) -> Ink {
    let [edge, mid, core, eye, glow] = match (tint, light) {
        (Tint::Auto, true) => return will_ink(Tint::Flame, true),
        (Tint::Auto, false) => return will_ink(Tint::Wisp, false),
        (Tint::Wisp, false) => [0x4f86e8, 0xaac8ff, 0xf4f6ff, 0x05050a, 0x6aa0ff],
        (Tint::Wisp, true) => [0x4084d6, 0x93b9ee, 0xf6f9ff, 0x1c1d1a, 0x4084d6],
        (Tint::Flame, false) => [0xf0652b, 0xf6a54a, 0xfff3d6, 0x1a0f08, 0xff8a4a],
        (Tint::Flame, true) => [0xd9541f, 0xf0a048, 0xfff4dc, 0x1c1d1a, 0xd9541f],
        (Tint::Foxfire, false) => [0x2fbf71, 0x9ee8bf, 0xf0fff4, 0x04100a, 0x3ad6b8],
        (Tint::Foxfire, true) => [0x16934f, 0x86d6a8, 0xf2fff6, 0x1c1d1a, 0x16934f],
        (Tint::Arcane, false) => [0x8b5cf6, 0xd4b4ff, 0xfbf4ff, 0x0a0514, 0xa070ff],
        (Tint::Arcane, true) => [0x6d3fd6, 0xbf9bf2, 0xfaf5ff, 0x1c1d1a, 0x6d3fd6],
    };
    let [warn, bad] = if light {
        [0xa87a12, 0xb03a2e]
    } else {
        [0xdcc85a, 0xdc5a5a]
    };
    Ink {
        edge: hex(edge),
        mid: hex(mid),
        core: hex(core),
        eye: hex(eye),
        glow: hex(glow),
        warn: hex(warn),
        bad: hex(bad),
    }
}

const fn hex(value: u32) -> Color {
    let [_, r, g, b] = value.to_be_bytes();
    rgb(r, g, b)
}
