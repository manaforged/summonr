use sigil::app::cell_width;

use super::{State, busy_info, find, frame, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

const TODO: &str =
    r#"{"items":[{"text":"Plan","status":"in_progress"},{"text":"Ship","status":"pending"}]}"#;

fn chat_side(lines: &[String], chat: usize) -> Vec<String> {
    lines
        .iter()
        .map(|line| line.chars().take(chat).collect())
        .collect()
}

#[test]
fn chrome_rows_start_at_the_gutter() {
    for cols in [40, 110, 200] {
        let seen = std::cell::RefCell::new(None);
        let full = frame_with(busy_info(), cols, 40, State::Busy, |app| {
            app.update(Msg::Event(Event::ToolCall {
                id: "todo".to_owned(),
                name: "todo".to_owned(),
                arguments: TODO.to_owned(),
            }));
            let (chat, side) = app.layout.get().split(cols);
            seen.replace(Some(if side == 0 { cols } else { chat + 1 }));
        });
        let width = usize::from(seen.into_inner().unwrap());
        let lines = chat_side(&full, width);
        let screen = full.join("\n");
        let rows = [
            ("busy", find(&lines, " · 0s")),
            (
                "todos",
                find(&lines, "Todos").or(find(&lines, "● auto").filter(|_| cols >= 120)),
            ),
            ("composer", find(&lines, "● auto")),
            (
                "footer",
                lines.iter().rposition(|line| line.contains("cache")),
            ),
        ];
        for (name, row) in rows {
            let row = row.unwrap_or_else(|| panic!("{cols}: no {name} row:\n{screen}"));
            let line = &lines[row];
            let left = line.chars().take_while(|c| *c == ' ').count();
            let right = width.saturating_sub(cell_width(line.trim_end()));
            assert_eq!(left, 2, "{cols}: {name} starts at col {left}:\n{screen}");
            let fixed = name == "composer";
            assert!(
                right >= 2 && (!fixed || right == 2),
                "{cols}: {name} ends {right} from the edge:\n{screen}"
            );
        }
    }
}

#[test]
fn the_transcript_opens_on_the_first_message_with_no_header_row() {
    let roomy = frame(80, 24, State::Idle);
    assert!(roomy[0].trim().is_empty(), "{}", roomy.join("\n"));
    assert!(
        roomy[1].contains("Review the change"),
        "{}",
        roomy.join("\n")
    );
    let footer = roomy.iter().rposition(|line| line.contains("main"));
    assert_eq!(footer, Some(22), "{}", roomy.join("\n"));
    let small = frame(40, 12, State::Idle);
    assert!(
        small[0].contains("Review the change"),
        "{}",
        small.join("\n")
    );
}
