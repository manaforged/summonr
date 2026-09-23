use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sigil::prelude::{Rgba8, Tone};
use sigil_tui::cell_width;
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

pub(super) fn overlay_shown(count: usize) -> usize {
    count.clamp(1, 8)
}

#[cfg(feature = "gui")]
pub(super) fn window_cells() {
    WINDOW.store(true, Ordering::Relaxed);
}

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

pub(super) use super::theme::palette;
pub(super) use sigil_tui::{Cell, Ink};

pub(super) const fn ink(colour: Rgba8) -> Ink {
    Ink::Exact(colour)
}

pub(super) fn shown(colour: Ink, fallback: Rgba8) -> Rgba8 {
    match colour {
        Ink::Bare => fallback,
        Ink::Of(tone) => match tone {
            Tone::Neutral => palette().text,
            Tone::Primary => palette().accent,
            Tone::Info => palette().cyan,
            Tone::Success => palette().green,
            Tone::Warn => palette().yellow,
            Tone::Error => palette().red,
        },
        Ink::Exact(colour) => colour,
    }
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

pub(super) fn ellipsize(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut shown = text.to_string();
    truncate(&mut shown, width.saturating_sub(1));
    shown.push('…');
    shown
}

pub(super) fn truncate(text: &mut String, width: usize) {
    let mut used = 0;
    let end = text.grapheme_indices(true).find_map(|(index, grapheme)| {
        used += cell_width(grapheme);
        (used > width).then_some(index)
    });
    if let Some(end) = end {
        text.truncate(end);
    }
}
