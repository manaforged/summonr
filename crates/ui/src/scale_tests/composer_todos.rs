use super::super::app::App;
use super::{State, busy_info, composer_top, find, frame_with};
use summonr_agent::durable::work::{WorkKind, WorkPhase, WorkProgress};
use summonr_agent::hub::{Fact, Intent, Msg};
use summonr_agent::tools::Event;

fn progress(ledger: Vec<String>, todos: Vec<summonr_agent::tools::Todo>) -> WorkProgress {
    WorkProgress {
        kind: WorkKind::Steps,
        objective: "Polish the interface".to_owned(),
        phase: WorkPhase::Execute,
        checks_failed: false,
        current: Some(1),
        total: 2,
        attempts: 1,
        rounds: 1,
        reason: None,
        ledger,
        todos,
        touched: Vec::new(),
    }
}

pub(super) fn todo(arguments: &str) -> Event {
    Event::Work {
        progress: progress(
            Vec::new(),
            summonr_agent::tools::todos_from_args(arguments).expect("todo arguments"),
        ),
    }
}

#[test]
fn work_sections_and_busy_status_share_one_row() {
    for (cols, rows) in [(100, 30), (60, 18)] {
        for (tasks, agents) in [(true, false), (false, true), (true, true)] {
            let lines = frame_with(busy_info(), cols, rows, State::Busy, |app| {
                if tasks {
                    app.update(Msg::Fact(Fact::Event(todo(
                        r#"{"items":[{"text":"Map decode owners","status":"active"}]}"#,
                    ))));
                }
                if agents {
                    super::children::task_call(app);
                    super::children::spawn(
                        app,
                        super::children::IDS[0],
                        "Inspect decode paths",
                        true,
                    );
                }
            });
            let shown = lines.join("\n");
            let label = if agents { "1 agent" } else { "1/1" };
            let row = find(&lines, label).expect("the status bar remains visible");
            if tasks && agents {
                assert!(lines[row].contains("1/1 Map decode owners"), "{shown}");
            }
            assert!(composer_top(&lines) > row, "{shown}");
            assert!(!shown.contains("Subagents"), "{shown}");
        }
    }
}

#[test]
fn live_rate_in_the_status_bar_disappears_for_tools_and_idle() {
    use summonr_agent::hub::Activity;
    let stream = |app: &App| {
        app.update(Msg::Fact(Fact::Event(Event::Delta {
            text: "Streaming output with enough characters for a visible rate.".to_string(),
        })));
    };
    let tool = |app: &App| {
        app.update(Msg::Fact(Fact::Event(Event::ToolCall {
            id: "rate-tool".to_string(),
            name: "read".to_string(),
            arguments: r#"{"path":"src/main.rs"}"#.to_string(),
        })));
    };
    let thinking = |app: &App| {
        app.update(Msg::Fact(Fact::Event(Event::ToolResult {
            id: "rate-tool".to_string(),
            success: true,
            mutated: false,
            output: "read complete".to_string(),
            millis: None,
            patch: None,
        })));
    };
    let idle = |app: &App| app.update(Msg::Fact(Fact::Busy(None)));
    let frames = super::screens(
        100,
        30,
        |app| {
            app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
            stream(app);
        },
        &[&tool, &thinking, &stream, &idle],
    );
    for (frame, expected) in frames.iter().zip([true, false, false, true, false]) {
        assert_eq!(
            frame.contains("tok/s"),
            expected,
            "stale or missing live rate: {frame}"
        );
    }
}

#[test]
fn a_work_plan_stays_above_the_composer_without_opening_a_pane() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(Event::Work {
            progress: progress(
                vec![
                    "[/] Keep tasks inline".to_owned(),
                    "[ ] Verify changes".to_owned(),
                ],
                Vec::new(),
            ),
        })));
        assert_eq!(app.layout.peek().secondary, None);
    });
    let shown = lines.join("\n");
    let tasks = find(&lines, "1/2").expect("the current task is visible");
    assert!(tasks < composer_top(&lines), "{shown}");
    assert!(lines[tasks].contains("Keep tasks inline"), "{shown}");
    assert!(!shown.contains("Ledger"), "{shown}");
}

#[test]
fn the_task_shortcut_opens_and_scrolls_the_status_popup() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(todo(
            r#"{"items":[{"text":"First task","status":"done"},{"text":"Second task","status":"active"},{"text":"Third task"},{"text":"Fourth task"},{"text":"Fifth task"},{"text":"Sixth task"},{"text":"Seventh task"},{"text":"Last task"}]}"#,
        ))));
        assert!(app.route_hotkey('t'));
        app.move_status(7);
        assert_eq!(app.status_list.cursor.peek(), 7);
        assert_eq!(app.layout.peek().secondary, None);
    });
    let shown = lines.join("\n");
    let tasks = find(&lines, "2/8 Second task").expect("the task strip is visible");
    let last = find(&lines, "Last task").expect("the last task is reachable by scrolling");
    assert!(tasks < last && shown.contains("Status"), "{shown}");
    assert!(!shown.contains("Ledger"), "{shown}");
}

#[test]
fn child_tasks_use_the_same_inline_strip() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: "01a104ac000000000000000000001111".to_owned(),
            event: Box::new(todo(
                r#"{"items":[{"text":"Check child result","status":"active"},{"text":"Report findings"}]}"#,
            )),
        })));
        assert!(app.route_hotkey('t'));
        assert_eq!(app.layout.peek().secondary, None);
    });
    let shown = lines.join("\n");
    let tasks = find(&lines, "1/2 Check child result").expect("the child task strip is visible");
    let active =
        find(&lines, "00001111 · Check child result").expect("the active child task is visible");
    let pending = find(&lines, "Report findings").expect("the pending child task is visible");
    assert!(
        tasks < active && active < pending && shown.contains("Status"),
        "{shown}"
    );
}

#[test]
fn the_task_shortcut_opens_status_without_active_tasks() {
    for arguments in [
        r#"{"items":[]}"#,
        r#"{"items":[{"text":"All finished","status":"done"}]}"#,
    ] {
        let lines = frame_with(busy_info(), 160, 40, State::Idle, |app: &App| {
            app.update(Msg::Fact(Fact::Event(todo(arguments))));
            assert_eq!(app.layout.peek().secondary, None);
            assert!(app.route_hotkey('t'));
            assert_eq!(app.layout.peek().secondary, None);
            assert_eq!(
                app.model.peek().status_view,
                super::super::model::StatusView::Tasks
            );
        });
        assert!(lines.join("\n").contains("Status"), "{}", lines.join("\n"));
    }
}

#[test]
fn todo_rows_of_a_child_leave_the_strip_when_the_child_finishes() {
    let child = "01a104ac000000000000000000001111".to_owned();
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: child.clone(),
            event: Box::new(todo(
                r#"{"items":[{"text":"Check child result","status":"active"}]}"#,
            )),
        })));
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: child.clone(),
            event: Box::new(Event::Assistant {
                text: "checked".to_owned(),
            }),
        })));
    });
    let shown = lines.join("\n");
    assert!(!shown.contains("Check child result"), "{shown}");
}

#[test]
fn todo_list_from_work_survives_a_parent_tool_call() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(todo(
            r#"{"items":[{"text":"Keep me","status":"active"}]}"#,
        ))));
        app.update(Msg::Fact(Fact::Event(Event::ToolCall {
            id: "t2".to_owned(),
            name: "todo".to_owned(),
            arguments: r#"{"items":[]}"#.to_owned(),
        })));
    });
    let shown = lines.join("\n");
    assert!(shown.contains("Keep me"), "{shown}");
}

#[test]
fn todo_rows_of_a_finished_child_stay_gone_after_a_late_update() {
    let child = "01a104ac000000000000000000002222".to_owned();
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: child.clone(),
            event: Box::new(Event::Assistant {
                text: "checked".to_owned(),
            }),
        })));
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: child.clone(),
            event: Box::new(todo(
                r#"{"items":[{"text":"Late child row","status":"active"}]}"#,
            )),
        })));
    });
    let shown = lines.join("\n");
    assert!(!shown.contains("Late child row"), "{shown}");
}

#[test]
fn a_numbered_prompt_does_not_open_the_task_strip() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        let mut single = progress(
            vec![
                "Two things:".to_owned(),
                "1. list the functions in calc.py".to_owned(),
                "2. no edits".to_owned(),
            ],
            Vec::new(),
        );
        single.kind = WorkKind::Single;
        app.update(Msg::Fact(Fact::Event(Event::Work { progress: single })));
    });
    let shown = lines.join("\n");
    assert!(find(&lines, "Tasks").is_none(), "{shown}");
}

#[test]
fn task_progress_updates_the_mounted_counter_and_clears_finished_work() {
    let tasks = |done| Event::Work {
        progress: progress(
            Vec::new(),
            (0..6)
                .map(|index| summonr_agent::tools::Todo {
                    text: format!("Task {index}"),
                    status: if index < done {
                        summonr_agent::tools::TodoStatus::Done
                    } else if index == done {
                        summonr_agent::tools::TodoStatus::Active
                    } else {
                        summonr_agent::tools::TodoStatus::Pending
                    },
                })
                .collect(),
        ),
    };
    let one = |app: &App| app.update(Msg::Fact(Fact::Event(tasks(1))));
    let five = |app: &App| app.update(Msg::Fact(Fact::Event(tasks(5))));
    let complete = |app: &App| app.update(Msg::Fact(Fact::Event(tasks(6))));
    let reset = |app: &App| app.update(Msg::Intent(Intent::Reset));
    let restart = |app: &App| {
        app.update(Msg::Fact(Fact::Session("another-session".to_owned())));
        app.update(Msg::Fact(Fact::Event(tasks(0))));
    };
    let frames = super::screens(
        100,
        30,
        |app| app.update(Msg::Fact(Fact::Event(tasks(0)))),
        &[&one, &five, &reset, &restart, &complete],
    );
    for (frame, count) in frames.iter().zip(["1/6", "2/6", "6/6"]) {
        assert!(frame.contains(count), "{frame}");
    }
    assert!(!frames[3].contains("/6"), "{}", frames[3]);
    assert!(frames[4].contains("1/6"), "{}", frames[4]);
    assert!(frames[5].contains("6/6 tasks"), "{}", frames[5]);
}
