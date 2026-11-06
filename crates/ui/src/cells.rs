use sigil::app::cell_width;

use summonr_agent::display::{self, Span};

/// Fits text to a width using the widths this terminal reports.
pub(crate) fn ellipsize(text: &str, span: impl Into<Span>) -> String {
    display::ellipsize_by(cell_width, text, span)
}

/// Fits text to a width from its end using the widths this terminal reports.
pub(crate) fn ellipsize_start(text: &str, width: usize) -> String {
    display::ellipsize_start_by(cell_width, text, width)
}

/// Wraps words to a width using the widths this terminal reports.
#[must_use]
pub(crate) fn lines_within(text: &str, width: usize) -> Vec<String> {
    display::lines_within_by(cell_width, text, width)
}
