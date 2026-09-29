use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sigil::app::cell_width;
use sigil::events::ScrollDelta;
use sigil::prelude::LayoutBuilderExt;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Rgba8;
use sigil::prelude::Tone;
use sigil::prelude::container;
use unicode_segmentation::UnicodeSegmentation;

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

pub(super) const GUTTER: f32 = 16.0;
pub(super) const GUTTER_COLS: usize = 2;

pub(super) fn gutter(child: NodeBuilder) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0)
                .padding_x(GUTTER),
        )
        .child(child)
}

pub(super) fn inner_cols(cols: usize) -> usize {
    cols.saturating_sub(2 * GUTTER_COLS)
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
        w: sigil::Host::CELL,
        h: sigil::Host::CELL,
        text: "",
    })
}

pub(super) const DIALOG_CELLS: f32 = 72.0;
pub(super) const WIDE_DIALOG_CELLS: f32 = 120.0;

pub(super) fn dialog_width(cols: u16, max_cells: f32) -> f32 {
    if WINDOW.load(Ordering::Relaxed) {
        520.0
    } else {
        (f32::from(cols.saturating_sub(4)) * cell().w).min(max_cells * cell().w)
    }
}

pub(super) fn overlay_gap() -> f32 {
    if WINDOW.load(Ordering::Relaxed) {
        2.0
    } else {
        0.0
    }
}

pub(super) fn overlay_field() -> f32 {
    if WINDOW.load(Ordering::Relaxed) {
        22.0
    } else {
        24.0
    }
}

pub(super) fn overlay_list_row() -> f32 {
    if WINDOW.load(Ordering::Relaxed) {
        22.0
    } else {
        cell().h
    }
}

pub(super) const DIALOG_CHROME: u16 = 16;
const DIALOG_LIST_CAP: usize = 14;

pub(super) fn overlay_shown(count: usize, rows: u16) -> usize {
    overlay_room(count, rows, DIALOG_CHROME).min(DIALOG_LIST_CAP)
}

pub(super) fn overlay_room(count: usize, rows: u16, used: u16) -> usize {
    let room = if WINDOW.load(Ordering::Relaxed) {
        8
    } else {
        usize::from(rows.saturating_sub(used)).max(1)
    };
    count.clamp(1, room)
}

#[cfg(feature = "gui")]
pub(super) fn window_cells() {
    WINDOW.store(true, Ordering::Relaxed);
}

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

pub(super) use super::theme::palette;
pub(super) use sigil::terminal::{Cell, Ink};

pub(super) const fn ink(colour: Rgba8) -> Ink {
    Ink::Exact(colour)
}

pub(super) fn shown(colour: Ink, fallback: Rgba8) -> Rgba8 {
    if let Ink::Exact(colour) = colour {
        return colour;
    }
    let Ink::Of(tone) = colour else {
        return fallback;
    };
    let palette = palette();
    [
        (Tone::Primary, palette.accent),
        (Tone::Info, palette.cyan),
        (Tone::Success, palette.green),
        (Tone::Warn, palette.yellow),
        (Tone::Error, palette.red),
    ]
    .into_iter()
    .find(|(named, _)| *named == tone)
    .map_or(palette.text, |(_, colour)| colour)
}

pub(super) fn scroll_lines(delta: ScrollDelta) -> f32 {
    if let ScrollDelta::Pixels(point) = delta {
        return point.y / cell().h;
    }
    let ScrollDelta::Lines(point) = delta else {
        return 0.0;
    };
    point.y
}

pub(super) fn write_at(
    cells: &mut [Cell],
    width: usize,
    left: usize,
    row: usize,
    text: &str,
    fg: Rgba8,
) {
    let room = width.saturating_sub(left);
    let start = row.saturating_mul(width).saturating_add(left);
    let Some(row) = cells.get_mut(start..start.saturating_add(room)) else {
        return;
    };
    let bg = palette().base;
    let cell = |text: &str| Cell {
        text: text.into(),
        fg: ink(fg),
        bg: ink(bg),
        bold: false,
        italic: false,
        ..Cell::blank()
    };
    let mut x = 0;
    for grapheme in text.graphemes(true) {
        let step = cell_width(grapheme);
        if x >= room || x.saturating_add(step) > room {
            return;
        }
        if step == 0 {
            continue;
        }
        row[x] = cell(grapheme);
        for pad in 1..step {
            row[x + pad] = cell("");
        }
        x += step;
    }
}
