use super::children::{IDS, act, spawn, task_call};
use super::{State, busy_info, frame_with};
use summonr_agent::hub::{Activity, Fact, Msg};
use summonr_agent::tools::Event;

fn row(lines: &[String]) -> String {
    lines
        .iter()
        .find(|line| line.contains("Benchmark"))
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
            "Thinking…",
            "Working…",
            "0/1 tasks",
            "1 blocked",
            "1/1 tasks",
            "✓",
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
        assert!(shown.contains("Thinking…"), "{shown}");
        assert!(!shown.contains("Polish the interface"), "{shown}");
        let waiting = |app: &super::super::app::App| {
            app.update(Msg::Fact(Fact::Event(Event::Approval {
                detail: Some("git diff --check".to_owned()),
                id: None,
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
        assert!(frames[1].contains("Waiting for you"), "{}", frames[1]);
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
    assert!(title(&wide).chars().count() <= 60, "{wide}");
    assert!(wide.contains("· Run hyperfine"), "{wide}");
    let narrow = row(&frame_with(busy_info(), 54, 40, State::Busy, |app| {
        running(app, &long)
    }));
    assert!(sigil::app::cell_width(narrow.trim_end()) <= 54, "{narrow}");
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
    assert!(after.contains("completed"), "{after}");
}

fn bar_line(frame: &str, needle: &str) -> String {
    frame
        .lines()
        .find(|line| line.contains(needle))
        .unwrap_or_else(|| panic!("no bar line with {needle}:\n{frame}"))
        .to_owned()
}

#[test]
fn an_idle_bar_with_failed_agents_states_each_fact_once() {
    let idle = |app: &super::super::app::App| {
        for id in &IDS[..2] {
            act(
                app,
                id,
                Event::Aborted {
                    reason: "budget reached".to_owned(),
                    code: summonr_agent::tools::AbortCode::TimeBudget,
                },
            );
        }
        act(
            app,
            IDS[2],
            Event::Assistant {
                text: "done".to_owned(),
            },
        );
        app.update(Msg::Fact(Fact::Busy(None)));
    };
    let frames = super::screens(
        100,
        30,
        |app| {
            app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
            app.update(Msg::Fact(Fact::Event(super::composer_todos::todo(
                r#"{"items":[{"text":"Read","status":"done"},{"text":"Fix","status":"done"}]}"#,
            ))));
            task_call(app);
            for id in IDS {
                spawn(app, id, "Check a module.", true);
            }
        },
        &[&idle],
    );
    let line = bar_line(&frames[1], "2/2 tasks");
    assert!(line.trim_start().starts_with('!'), "{line}");
    assert!(
        line.contains("3 agents") && line.contains("2 failed"),
        "{line}"
    );
    assert_eq!(line.matches("failed").count(), 1, "{line}");
    assert!(
        !line.contains("Agents failed") && !line.contains("(0s"),
        "{line}"
    );
}

#[test]
fn the_rate_shows_while_thinking_and_writing_but_not_during_tools() {
    let think = |app: &super::super::app::App| {
        app.update(Msg::Fact(Fact::Event(Event::Reasoning {
            text: "Considering the options with plenty of characters to measure.".to_owned(),
        })));
    };
    let tool = |app: &super::super::app::App| {
        app.update(Msg::Fact(Fact::Event(Event::ToolCall {
            id: "rate-tool".to_owned(),
            name: "read".to_owned(),
            arguments: r#"{"path":"src/main.rs"}"#.to_owned(),
        })));
    };
    let frames = super::screens(
        100,
        30,
        |app| app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn)))),
        &[&think, &tool],
    );
    let thinking = bar_line(&frames[1], "Thinking…");
    assert!(thinking.contains("tok/s"), "{thinking}");
    assert!(thinking.contains("esc to interrupt"), "{thinking}");
    let working = bar_line(&frames[2], "Working…");
    assert!(!working.contains("tok/s"), "{working}");
}

#[test]
fn a_busy_bar_fits_and_keeps_its_clock_at_every_width() {
    for cols in [40_u16, 54, 60, 80, 100, 140] {
        let frames = super::screens(
            cols,
            30,
            |app| {
                app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
                app.update(Msg::Fact(Fact::Event(super::composer_todos::todo(
                    r#"{"items":[{"text":"Fix the auth refresh race in the token store","status":"active"},{"text":"Ship"}]}"#,
                ))));
                task_call(app);
                for id in IDS {
                    spawn(app, id, "Check a module.", true);
                }
            },
            &[],
        );
        let line = bar_line(&frames[0], "(0s");
        assert!(
            sigil::app::cell_width(line.trim_end()) <= usize::from(cols),
            "{cols}: {line}"
        );
        assert!(
            line.contains("…") || line.contains("Working"),
            "{cols}: {line}"
        );
        assert!(!line.trim_end().ends_with('·'), "{cols}: {line}");
        if cols >= 80 {
            assert!(line.contains("1/2 Fix the auth"), "{cols}: {line}");
            assert!(line.contains("3 agents"), "{cols}: {line}");
        }
    }
}
