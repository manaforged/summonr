use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sigil::prelude::Rgba8;
use smol_str::SmolStr;

/// Mix two colors. One copy: the surfaces differ, the arithmetic does not.
pub(super) fn blend(from: Rgba8, to: Rgba8, amount: f32) -> Rgba8 {
    let channel = |a: u8, b: u8| {
        (i16::from(a) + ((i16::from(b) - i16::from(a)) as f32 * amount).round() as i16)
            .clamp(0, 255) as u8
    };
    rgb(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
    )
}

static WINDOW: AtomicBool = AtomicBool::new(false);

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

#[cfg(feature = "gui")]
pub(super) fn window_cells() {
    WINDOW.store(true, Ordering::Relaxed);
}

pub(super) fn cell_aspect() -> f32 {
    if WINDOW.load(Ordering::Relaxed) {
        1.0
    } else {
        2.0
    }
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
