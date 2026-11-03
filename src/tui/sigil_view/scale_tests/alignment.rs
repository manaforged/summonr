use super::children::{IDS, act, spawn, task_call};
use super::{State, busy_info, frame_with};
use crate::tools::Event;

fn usage(tokens: u32) -> Event {
    Event::Usage {
        prompt_tokens: tokens,
        completion_tokens: 0,
        reasoning_tokens: 0,
        cached_tokens: 0,
        cache_write_tokens: 0,
        millis: None,
        prefix_hash: String::new(),
        cost: None,
    }
}

#[test]
fn subagents_share_one_status_row_at_every_width() {
    for (cols, rows) in [(40, 18), (84, 25), (100, 30), (140, 40)] {
        let lines = frame_with(busy_info(), cols, rows, State::Busy, |app| {
            task_call(app);
            for (id, assignment) in IDS.iter().zip([
                "Fix total() in pricing.py so it rounds to cents.",
                "Add a --dry-run flag to cli.py.",
                "Document the CLI in README.md.",
            ]) {
                spawn(app, id, assignment, true);
            }
            act(app, IDS[0], usage(1_400));
            act(app, IDS[2], usage(25_000));
        });
        let shown = lines.join("\n");
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.contains("3 agents"))
                .count(),
            1,
            "{shown}"
        );
        assert!(!shown.contains("Subagents"), "{shown}");
        assert!(!shown.contains("25K tok"), "{shown}");
    }
}
