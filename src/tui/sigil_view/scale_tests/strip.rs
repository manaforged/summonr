use super::children::{IDS, act, spawn, task_call};
use super::{State, busy_info, frame_with};
use crate::tools::Event;

fn row(lines: &[String]) -> String {
    lines
        .iter()
        .find(|line| {
            line.trim_start().starts_with("○ Subagent")
                || line.trim_start().starts_with("● Subagent")
        })
        .cloned()
        .unwrap_or_default()
}

fn running(app: &super::super::app::App, assignment: &str) {
    task_call(app);
    spawn(app, IDS[0], assignment, true);
    act(
        app,
        IDS[0],
        Event::ToolCall {
            id: "c".to_owned(),
            name: "run_command".to_owned(),
            arguments: r#"{"command":"hyperfine ./summonr"}"#.to_owned(),
        },
    );
}

fn title(row: &str) -> String {
    let after = row.split_once("Subagent ").map_or("", |(_, rest)| rest);
    after.split(" · ").next().unwrap_or_default().to_owned()
}

#[test]
fn a_long_subagent_title_leaves_its_share_to_the_rest_of_the_row() {
    let long = format!("Benchmark {} startup and RSS", "every module ".repeat(30));
    let wide = row(&frame_with(busy_info(), 100, 40, State::Busy, |app| {
        running(app, &long)
    }));
    assert!(title(&wide).chars().count() <= 40, "{wide}");
    let narrow = row(&frame_with(busy_info(), 54, 40, State::Busy, |app| {
        running(app, &long)
    }));
    assert!(narrow.trim_end().ends_with("0s · 0 tok"), "{narrow}");
    assert!(narrow.contains("· Run hyperfine ./summ…"), "{narrow}");
}

#[test]
fn a_subagent_row_redraws_when_it_finishes() {
    let after = row(&frame_with(busy_info(), 140, 40, State::Busy, |app| {
        running(app, "Benchmark startup and RSS.");
        app.toggle_agents();
        act(
            app,
            IDS[0],
            Event::Assistant {
                text: "startup 0.28 s".to_owned(),
            },
        );
    }));
    assert!(after.trim_start().starts_with("● Subagent"), "{after}");
}
