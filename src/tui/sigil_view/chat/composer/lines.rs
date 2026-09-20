use super::*;

#[expect(dead_code, reason = "empty transcript copy stays next to composer")]
pub(in crate::tui::sigil_view) fn empty_hint(width: usize) -> &'static str {
    [
        "ask anything · /login setup · @file attaches · !cmd runs · /help",
        "ask anything · /login · @file · /help",
        "ask anything · /login",
        "ask anything",
    ]
    .into_iter()
    .find(|hint| cell_width(hint) <= width)
    .unwrap_or("")
}
