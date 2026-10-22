use super::children::{IDS, act, spawn, task_call};
use super::{State, busy_info, frame_with};
use crate::tools::Event;

fn usage(tokens: u32) -> Event {
    Event::Usage {
        prompt_tokens: tokens,
        completion_tokens: 0,
        reasoning_tokens: 0,
        cached_tokens: 0,
        millis: None,
        prefix_hash: String::new(),
        cost: None,
    }
}

fn columns(line: &str) -> Vec<usize> {
    line.chars()
        .enumerate()
        .filter(|(_, ch)| *ch == '·')
        .map(|(at, _)| at)
        .collect()
}

#[test]
fn subagent_strip_rows_share_their_columns() {
    for (cols, rows) in [(84, 25), (100, 30), (140, 40)] {
        let lines = frame_with(busy_info(), cols, rows, State::Busy, |app| {
            task_call(app);
            spawn(
                app,
                IDS[0],
                "Fix total() in pricing.py so it rounds to cents.",
                true,
            );
            spawn(app, IDS[1], "Add a --dry-run flag to cli.py.", true);
            spawn(
                app,
                IDS[2],
                "Document the CLI in README.md with an example for every flag.",
                true,
            );
            act(
                app,
                IDS[0],
                Event::ToolCall {
                    id: "c".to_owned(),
                    name: "read".to_owned(),
                    arguments: r#"{"path":"pricing.py"}"#.to_owned(),
                },
            );
            act(app, IDS[0], usage(1_400));
            act(app, IDS[2], usage(25_000));
        });
        let strip: Vec<&String> = lines
            .iter()
            .filter(|line| line.contains("○ Subagent"))
            .collect();
        let shown = lines.join("\n");
        assert_eq!(strip.len(), 3, "{cols}x{rows}: three strip rows:\n{shown}");
        let separators: Vec<Vec<usize>> = strip.iter().map(|line| columns(line)).collect();
        assert!(
            separators.windows(2).all(|pair| pair[0] == pair[1]),
            "{cols}x{rows}: separators at {separators:?}:\n{shown}"
        );
        let ends: Vec<usize> = strip
            .iter()
            .map(|line| line.trim_end().chars().count())
            .collect();
        assert!(
            ends.windows(2).all(|pair| pair[0] == pair[1]),
            "{cols}x{rows}: rows end at {ends:?}:\n{shown}"
        );
    }
}
