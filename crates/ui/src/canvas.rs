use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sigil::events::ScrollDelta;
use sigil::prelude::LayoutStyle;
use sigil::prelude::NodeBuilder;
use sigil::prelude::Rgba8;
use sigil::prelude::Tone;
use sigil::prelude::Widget;
use sigil::prelude::container;

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
pub(super) const ROOMY_ROWS: u16 = 24;

pub(super) fn edge(rows: u16) -> f32 {
    if rows >= ROOMY_ROWS { cell().h } else { 0.0 }
}

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

pub(crate) static WINDOW: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_window() -> bool {
    WINDOW.load(Ordering::Relaxed) || sigil::terminal::pixels()
}

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
const WINDOW_INSET: f32 = 24.0;
const WINDOW_ASPECT_MIN: f32 = 1.15;
const WINDOW_ASPECT_MAX: f32 = 1.8;
const WINDOW_MIN_W: f32 = 420.0;
const WINDOW_MIN_H: f32 = 280.0;
const WINDOW_LIST_CHROME: f32 = 112.0;

pub(super) fn window_frame((cols, rows): (u16, u16)) -> (f32, f32) {
    let cell = cell();
    let avail_w = (f32::from(cols) * cell.w - 2.0 * WINDOW_INSET).max(WINDOW_MIN_W);
    let avail_h = (f32::from(rows) * cell.h - 2.0 * WINDOW_INSET).max(WINDOW_MIN_H);
    let aspect = (avail_w / avail_h).clamp(WINDOW_ASPECT_MIN, WINDOW_ASPECT_MAX);
    let height_from_width = avail_w / aspect;
    if height_from_width <= avail_h {
        (avail_w, height_from_width)
    } else {
        (avail_h * aspect, avail_h)
    }
}

pub(super) fn frame_body(viewport: (u16, u16)) -> f32 {
    (window_frame(viewport).1 - WINDOW_LIST_CHROME).max(overlay_list_row())
}

pub(super) fn frame_rows(viewport: (u16, u16)) -> usize {
    (frame_body(viewport) / overlay_list_row()).floor() as usize
}
pub(crate) const fn frame_border() -> i16 {
    1
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
const PAGE_CHROME: u16 = 5;

const PICKER_ROWS: u16 = 49;

pub(super) fn picker_rows(rows: u16) -> u16 {
    rows.min(PICKER_ROWS)
}

pub(super) fn page_rows(rows: u16) -> u16 {
    rows.saturating_sub(PAGE_CHROME).max(8)
}

pub(super) fn list_rows(viewport: (u16, u16), chrome: u16) -> usize {
    if is_window() {
        frame_rows(viewport)
    } else {
        usize::from(page_rows(viewport.1).saturating_sub(chrome)).max(1)
    }
}

pub(super) fn overlay_shown(count: usize, viewport: (u16, u16)) -> usize {
    let room = if is_window() {
        frame_rows(viewport)
    } else {
        usize::from(viewport.1.saturating_sub(DIALOG_CHROME)).max(1)
    };
    count.clamp(1, room)
}

pub(super) const fn rgb(r: u8, g: u8, b: u8) -> Rgba8 {
    Rgba8 { r, g, b, a: 255 }
}

pub(super) use super::theme::palette;
pub(super) use sigil::terminal::{Cell, Ink};

pub(super) const fn ink(colour: Rgba8) -> Ink {
    if colour.a == 0 {
        Ink::Bare
    } else {
        Ink::Exact(colour)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    struct Flag(bool);

    impl Drop for Flag {
        fn drop(&mut self) {
            WINDOW.store(self.0, Ordering::Relaxed);
        }
    }

    #[test]
    fn a_window_menu_fills_the_pane() {
        let _flag = Flag(WINDOW.swap(true, Ordering::Relaxed));
        let viewport = (160, 100);
        let frame = window_frame(viewport);
        let pane_w = f32::from(viewport.0) * cell().w - 48.0;
        let pane_h = f32::from(viewport.1) * cell().h - 48.0;
        assert!((frame.0 - pane_w).abs() < 1.0, "width {}", frame.0);
        assert!((frame.1 - pane_h).abs() < 1.0, "height {}", frame.1);
        let wide = window_frame((400, 80));
        let aspect = wide.0 / wide.1;
        assert!(aspect <= 1.81, "{aspect}");
        assert!(
            (wide.1 - (80.0 * cell().h - 48.0)).abs() < 1.0,
            "{}",
            wide.1
        );
        let width = crate::ui::Size::Normal.width(viewport);
        assert!((width - frame.0).abs() < 1.0, "{width}");
    }
}
