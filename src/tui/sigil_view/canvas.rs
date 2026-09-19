use std::ops::Range;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sigil::prelude::{Rgba8, Tone};
use sigil_tui::cell_width;
use unicode_segmentation::UnicodeSegmentation;

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

#[derive(Clone)]
pub(super) struct Span {
    pub(super) text: String,
    pub(super) fg: Rgba8,
    pub(super) bg: Rgba8,
    pub(super) bold: bool,
    pub(super) italic: bool,
    pub(super) source: Option<Source>,
    pub(super) diff_layout: Option<crate::prefs::DiffLayout>,
}

#[derive(Clone)]
pub(super) enum Source {
    Text(Range<usize>),
    Block(Range<usize>),
}

impl Source {
    pub(super) fn range(&self) -> &Range<usize> {
        match self {
            Self::Text(range) | Self::Block(range) => range,
        }
    }
}

impl Span {
    pub(super) fn text(text: impl Into<String>) -> Self {
        Self::styled(text, palette().text)
    }

    pub(super) fn styled(text: impl Into<String>, fg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg: palette().base,
            bold: false,
            italic: false,
            source: None,
            diff_layout: None,
        }
    }

    pub(super) fn surface(text: impl Into<String>, fg: Rgba8, bg: Rgba8) -> Self {
        Self {
            text: text.into(),
            fg,
            bg,
            bold: false,
            italic: false,
            source: None,
            diff_layout: None,
        }
    }

    pub(super) fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    pub(super) fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub(super) fn width(&self) -> usize {
        cell_width(self.text.as_str())
    }

    pub(super) fn source(mut self, start: usize) -> Self {
        self.source = Some(Source::Text(start..start + self.text.len()));
        self
    }

    pub(super) fn slice(&self, range: Range<usize>) -> Self {
        let source = match &self.source {
            Some(Source::Text(source)) => Some(Source::Text(
                source.start + range.start..source.start + range.end,
            )),
            _ => self.source.clone(),
        };
        Self {
            text: self.text[range].to_string(),
            fg: self.fg,
            bg: self.bg,
            bold: self.bold,
            italic: self.italic,
            source,
            diff_layout: self.diff_layout,
        }
    }
}

pub(super) struct Line {
    pub(super) spans: Vec<Span>,
    pub(super) bg: Rgba8,
}

impl From<Vec<Span>> for Line {
    fn from(spans: Vec<Span>) -> Self {
        Self {
            spans,
            bg: palette().base,
        }
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
    write_line_at(
        cells,
        width,
        left,
        width.saturating_sub(left),
        row,
        &[Span::styled(text, fg)],
    );
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

pub(super) fn write_line_at(
    cells: &mut [Cell],
    stride: usize,
    left: usize,
    width: usize,
    y: usize,
    line: &[Span],
) {
    let start = y.saturating_mul(stride).saturating_add(left);
    let Some(row) = cells.get_mut(start..start.saturating_add(width)) else {
        return;
    };
    let mut x = 0;
    for span in line {
        for text in span.text.graphemes(true) {
            let step = cell_width(text);
            if x >= width || x.saturating_add(step) > width {
                return;
            }
            if step == 0 {
                continue;
            }
            row[x] = Cell {
                text: text.into(),
                fg: ink(span.fg),
                bg: ink(span.bg),
                bold: span.bold,
                italic: span.italic,
            };
            for pad in 1..step {
                if x + pad < width {
                    row[x + pad] = Cell {
                        text: "".into(),
                        fg: ink(span.fg),
                        bg: ink(span.bg),
                        bold: span.bold,
                        italic: span.italic,
                    };
                }
            }
            x += step;
        }
    }
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
