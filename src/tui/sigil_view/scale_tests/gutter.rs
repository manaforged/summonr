use sigil::app::cell_width;

use super::{State, busy_info, find, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

const TODO: &str =
    r#"{"items":[{"text":"Plan","status":"in_progress"},{"text":"Ship","status":"pending"}]}"#;

fn chat_side(lines: &[String], cols: u16) -> (Vec<String>, usize) {
    let border = lines
        .last()
        .and_then(|line| line.chars().position(|c| c == '│'));
    let lines = lines
        .iter()
        .map(|line| line.chars().take(border.unwrap_or(line.len())).collect())
        .collect();
    (lines, border.unwrap_or(usize::from(cols)))
}

#[test]
fn chrome_rows_start_at_the_gutter() {
    for cols in [40, 110, 200] {
        let full = frame_with(busy_info(), cols, 40, State::Busy, |app| {
            app.update(Msg::Event(Event::ToolCall {
                id: "todo".to_owned(),
                name: "todo".to_owned(),
                arguments: TODO.to_owned(),
            }));
        });
        let (lines, width) = chat_side(&full, cols);
        let screen = full.join("\n");
        let rows = [
            ("header", find(&lines, "summonr")),
            ("busy", find(&lines, " · 0s")),
            (
                "todos",
                find(&lines, "Todos").or((cols >= 120).then_some(0)),
            ),
            ("composer", find(&lines, "● auto")),
            (
                "footer",
                lines.iter().rposition(|line| line.contains("ctx")),
            ),
        ];
        for (name, row) in rows {
            let row = row.unwrap_or_else(|| panic!("{cols}: no {name} row:\n{screen}"));
            let line = &lines[row];
            let left = line.chars().take_while(|c| *c == ' ').count();
            let right = width.saturating_sub(cell_width(line.trim_end()));
            assert_eq!(left, 2, "{cols}: {name} starts at col {left}:\n{screen}");
            let fixed = matches!(name, "header" | "composer");
            assert!(
                right >= 2 && (!fixed || right == 2),
                "{cols}: {name} ends {right} from the edge:\n{screen}"
            );
        }
    }
}
