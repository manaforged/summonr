use super::children::{IDS, act, spawn, task_call};
use super::{State, busy_info, frame_with};
use crate::tools::Event;
use crate::tui::{Activity, Fact, Msg};

fn row(lines: &[String]) -> String {
    lines
        .iter()
        .find(|line| line.contains("tok"))
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
    let after = row.trim_start().trim_start_matches(['✳', '●']).trim_start();
    let after = after.strip_prefix("Subagent").unwrap_or(after).trim_start();
    after.split(" · ").next().unwrap_or_default().to_owned()
}

#[test]
fn status_labels_follow_active_work_instead_of_tools_or_stale_objectives() {
    let task = |app: &super::super::app::App, status: &str| {
        let arguments = serde_json::json!({
            "items": [{"text": "Verify the change", "status": status}]
        });
        app.update(Msg::Fact(Fact::Event(super::composer_todos::todo(
            &arguments.to_string(),
        ))));
    };
    let tool = |app: &super::super::app::App| {
        app.update(Msg::Fact(Fact::Event(Event::ToolCall {
            id: "status-tool".to_owned(),
            name: "run_command".to_owned(),
            arguments: r#"{"command":"git diff --check"}"#.to_owned(),
        })));
    };
    let pending = |app: &super::super::app::App| task(app, "pending");
    let blocked = |app: &super::super::app::App| task(app, "blocked");
    let done = |app: &super::super::app::App| task(app, "done");
    let idle = |app: &super::super::app::App| app.update(Msg::Fact(Fact::Busy(None)));
    for cols in [60, 100] {
        let frames = super::screens(
            cols,
            30,
            |app| {
                app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
                task(app, "active");
            },
            &[&tool, &pending, &blocked, &done, &idle],
        );
        for (frame, expected) in frames.iter().zip([
            "Verify the change",
            "Verify the change",
            "working",
            "Blocked",
            "working",
            "Complete",
        ]) {
            let lines: Vec<_> = frame.lines().collect();
            let at = lines
                .iter()
                .position(|line| line.contains("/1"))
                .expect("rendered task status");
            let status = lines[at.saturating_sub(1)..=at].join("\n");
            println!("{cols} cols: {status}");
            assert!(status.contains(expected), "expected {expected}: {frame}");
            assert!(!status.contains("run_command"), "{frame}");
            assert!(!status.contains("git diff"), "{frame}");
            assert!(!status.contains("Polish the interface"), "{frame}");
            if expected == "Blocked" {
                assert!(status.contains("1 blocked"), "{frame}");
            }
        }
    }
}

#[test]
fn status_fallbacks_and_long_labels_keep_waiting_visible() {
    for cols in [60, 100] {
        let lines = frame_with(busy_info(), cols, 30, State::Busy, |app| {
            app.update(Msg::Fact(Fact::Event(super::composer_todos::todo(
                r#"{"items":[]}"#,
            ))));
        });
        let shown = lines.join("\n");
        assert!(shown.contains("working"), "{shown}");
        assert!(!shown.contains("Polish the interface"), "{shown}");
        let waiting = |app: &super::super::app::App| {
            app.update(Msg::Fact(Fact::Event(Event::Approval {
                detail: Some("git diff --check".to_owned()),
                persist: false,
                dialog: 1,
            })));
        };
        let frames = super::screens(
            cols,
            30,
            |app| {
                app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
                let arguments = serde_json::json!({
                    "items": [{
                        "text": "Verify the change and ".repeat(10),
                        "status": "active"
                    }]
                });
                app.update(Msg::Fact(Fact::Event(super::composer_todos::todo(
                    &arguments.to_string(),
                ))));
            },
            &[&waiting],
        );
        assert!(frames[0].contains("Verify"), "{}", frames[0]);
        assert!(frames[1].contains("Waiting for input"), "{}", frames[1]);
        for frame in frames {
            for line in frame.lines() {
                assert!(
                    sigil::app::cell_width(line.trim_end()) <= usize::from(cols),
                    "overflow: {frame}"
                );
            }
        }
    }
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
    assert!(narrow.contains("· Run hyperfine ./su"), "{narrow}");
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
    assert!(after.trim_start().starts_with("●"), "{after}");
}
