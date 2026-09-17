#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Window {
    pub(super) start: usize,
    pub(super) line: usize,
    pub(super) col: usize,
}

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
