use std::sync::OnceLock;

use sigil::prelude::*;

/// Cell geometry in layout units plus the text class that fits it. The
/// terminal host keeps Sigil's square cell; the desktop host installs a
/// font-shaped cell before its first frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Metrics {
    pub(super) w: f32,
    pub(super) h: f32,
    pub(super) text: &'static str,
}

static METRICS: OnceLock<Metrics> = OnceLock::new();

pub(super) fn cell() -> Metrics {
    *METRICS.get_or_init(|| Metrics {
        w: sigil_tui::Host::CELL,
        h: sigil_tui::Host::CELL,
        text: "",
    })
}

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

pub(super) const BASE: Rgba8 = rgb(12, 12, 20);
pub(super) const TEXT: Rgba8 = rgb(200, 200, 205);
pub(super) const GRAY: Rgba8 = rgb(160, 160, 160);
pub(super) const DIM: Rgba8 = rgb(90, 90, 90);
pub(super) const GREEN: Rgba8 = rgb(80, 200, 120);
pub(super) const RED: Rgba8 = rgb(225, 95, 95);
pub(super) const CYAN: Rgba8 = rgb(90, 200, 220);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) ch: char,
    pub(super) fg: Rgba8,
    pub(super) bg: Rgba8,
    pub(super) bold: bool,
    pub(super) italic: bool,
}

impl Cell {
    pub(super) const fn blank() -> Self {
        Self {
            ch: ' ',
            fg: TEXT,
            bg: BASE,
            bold: false,
            italic: false,
        }
    }
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
