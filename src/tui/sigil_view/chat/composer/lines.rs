use super::*;
use sigil::prelude::Rgba8;

pub(in crate::tui::sigil_view) fn attachment_line(attached: usize) -> Vec<Span> {
    let noun = if attached == 1 { "image" } else { "images" };
    vec![
        Span::styled("📷 ", palette().gray),
        Span::styled(format!("{attached} {noun} attached"), palette().accent),
        Span::styled(" · enter sends with your prompt", palette().gray),
    ]
}

pub(in crate::tui::sigil_view) fn busy_line(
    status: &str,
    width: usize,
    accent: Rgba8,
) -> Vec<Span> {
    let shown = ellipsize(status, width);
    let (spinner, rest) = shown.split_at(shown.find(' ').unwrap_or(shown.len()));
    vec![
        Span::styled(spinner.to_string(), accent),
        Span::styled(rest.to_string(), palette().text),
    ]
}

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
