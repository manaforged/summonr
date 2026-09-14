use std::sync::OnceLock;

use sigil::prelude::*;
use smol_str::SmolStr;

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

pub(super) use super::theme::palette;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Cell {
    pub(super) text: SmolStr,
    pub(super) fg: Rgba8,
    pub(super) bg: Rgba8,
    pub(super) bold: bool,
    pub(super) italic: bool,
}

impl Cell {
    pub(super) fn blank() -> Self {
        let colors = palette();
        Self {
            text: SmolStr::new_inline(" "),
            fg: colors.text,
            bg: colors.base,
            bold: false,
            italic: false,
        }
    }
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
