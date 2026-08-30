/// Where the caret of an edited buffer sits and which lines to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Window {
    /// First line on show.
    pub(super) start: usize,
    /// Caret line, 0-based.
    pub(super) line: usize,
    /// Caret column in characters.
    pub(super) col: usize,
}

/// Places the caret (a byte offset) and keeps its line inside `body` rows,
/// moving `scroll` only as far as needed.
pub(super) fn window(text: &str, caret: usize, scroll: usize, body: usize) -> Window {
    let caret = caret.min(text.len());
    let before = &text[..caret];
    let line = before.matches('\n').count();
    let col = before.rsplit('\n').next().unwrap_or("").chars().count();
    let body = body.max(1);
    let start = if line < scroll {
        line
    } else if line >= scroll + body {
        line + 1 - body
    } else {
        scroll
    };
    Window { start, line, col }
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod tests;
