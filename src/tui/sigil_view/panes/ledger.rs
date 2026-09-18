use super::super::chat;
use super::{App, Line, palette};
use sigil::prelude::Rgba8;

pub(super) fn lines(app: &App) -> Vec<Line> {
    let model = app.model.borrow();
    let accent = chat::accent(&model.prefs.theme);
    let done = model
        .ledger
        .iter()
        .filter(|line| line.trim_start().starts_with("[x] ") || line.trim_start().starts_with("✓ "))
        .count();
    let total = model.ledger.len();
    let title = if total == 0 {
        "todos".to_string()
    } else {
        format!("todos · {done}/{total}")
    };
    let mut lines = vec![
        Line::new(title, accent),
        Line::new(String::new(), palette().text),
    ];
    if model.ledger.is_empty() {
        lines.push(Line::new("no todos yet".to_string(), palette().dim));
        return lines;
    }
    for line in &model.ledger {
        lines.push(plan_line(line, accent));
    }
    lines
}

pub(in crate::tui::sigil_view) fn item(line: &str) -> (char, &str) {
    let body = line.trim_start();
    for (prefix, mark) in [
        ("[x] ", '✓'),
        ("✓ ", '✓'),
        ("[/] ", '●'),
        ("● ", '●'),
        ("[ ] ", '○'),
        ("○ ", '○'),
        ("[!] ", '!'),
    ] {
        if let Some(rest) = body.strip_prefix(prefix) {
            return (mark, rest);
        }
    }
    (' ', body)
}

fn plan_line(line: &str, accent: Rgba8) -> Line {
    let indent = line.len() - line.trim_start().len();
    let (mark, text) = item(line);
    let fg = match mark {
        '✓' => palette().dim,
        '●' => accent,
        '!' => palette().error,
        _ => palette().text,
    };
    Line::new(format!("{}{mark} {text}", " ".repeat(indent)), fg)
}
