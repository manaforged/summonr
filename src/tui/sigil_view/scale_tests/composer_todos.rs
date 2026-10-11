use super::super::app::App;
use super::{State, busy_info, composer_top, find, frame_with};
use crate::durable::work::{WorkKind, WorkPhase, WorkProgress};
use crate::tools::Event;
use crate::tui::Msg;

fn progress(ledger: Vec<String>, todos: Vec<crate::tools::Todo>) -> WorkProgress {
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
            crate::tools::todos_from_args(arguments).expect("todo arguments"),
        ),
    }
}

#[test]
fn a_work_plan_stays_above_the_composer_without_opening_a_pane() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Event(Event::Work {
            progress: progress(
                vec![
                    "[/] Keep tasks inline".to_owned(),
                    "[ ] Verify changes".to_owned(),
                ],
                Vec::new(),
            ),
        }));
        assert_eq!(app.layout.peek().secondary, None);
    });
    let shown = lines.join("\n");
    let tasks = find(&lines, "Todos 0/2").expect("the task strip is visible");
    assert!(tasks < composer_top(&lines), "{shown}");
    assert!(lines[tasks].contains("Keep tasks inline"), "{shown}");
    assert!(!shown.contains("Ledger"), "{shown}");
}

#[test]
fn the_task_shortcut_expands_and_scrolls_the_inline_list() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Event(todo(
            r#"{"items":[{"text":"First task","status":"done"},{"text":"Second task","status":"active"},{"text":"Third task"},{"text":"Fourth task"},{"text":"Fifth task"},{"text":"Sixth task"},{"text":"Seventh task"},{"text":"Last task"}]}"#,
        )));
        assert!(app.route_hotkey('t'));
        assert!(app.scroll_todos(2));
        assert_eq!(app.layout.peek().secondary, None);
    });
    let shown = lines.join("\n");
    let tasks = find(&lines, "Todos 1/8").expect("the task strip is visible");
    let last = find(&lines, "◦ Last task").expect("the last task is reachable by scrolling");
    assert!(tasks < last && last < composer_top(&lines), "{shown}");
    assert!(!shown.contains("Ledger"), "{shown}");
}

#[test]
fn child_tasks_use_the_same_inline_strip() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Event(Event::Child {
            id: "01a104ac000000000000000000001111".to_owned(),
            event: Box::new(todo(
                r#"{"items":[{"text":"Check child result","status":"active"},{"text":"Report findings"}]}"#,
            )),
        }));
        assert!(app.route_hotkey('t'));
        assert_eq!(app.layout.peek().secondary, None);
    });
    let shown = lines.join("\n");
    let tasks = find(&lines, "Todos 0/2").expect("the child task strip is visible");
    let active = find(&lines, "Check child result").expect("the active child task is visible");
    let pending = find(&lines, "Report findings").expect("the pending child task is visible");
    assert!(
        tasks < active && active < pending && pending < composer_top(&lines),
        "{shown}"
    );
}

#[test]
fn the_task_shortcut_still_opens_the_ledger_without_active_tasks() {
    for arguments in [
        r#"{"items":[]}"#,
        r#"{"items":[{"text":"All finished","status":"done"}]}"#,
    ] {
        let lines = frame_with(busy_info(), 160, 40, State::Idle, |app: &App| {
            app.update(Msg::Event(todo(arguments)));
            assert_eq!(app.layout.peek().secondary, None);
            assert!(app.route_hotkey('t'));
            assert_eq!(
                app.layout.peek().secondary,
                Some(super::super::pane::Pane::Ledger)
            );
        });
        assert!(lines.join("\n").contains("Ledger"), "{}", lines.join("\n"));
    }
}

#[test]
fn todo_rows_of_a_child_leave_the_strip_when_the_child_finishes() {
    let child = "01a104ac000000000000000000001111".to_owned();
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Event(Event::Child {
            id: child.clone(),
            event: Box::new(todo(
                r#"{"items":[{"text":"Check child result","status":"active"}]}"#,
            )),
        }));
        app.update(Msg::Event(Event::Child {
            id: child.clone(),
            event: Box::new(Event::Assistant {
                text: "checked".to_owned(),
            }),
        }));
    });
    let shown = lines.join("\n");
    assert!(!shown.contains("Check child result"), "{shown}");
}

#[test]
fn todo_list_from_work_survives_a_parent_tool_call() {
    let lines = frame_with(busy_info(), 160, 40, State::Busy, |app| {
        app.update(Msg::Event(todo(
            r#"{"items":[{"text":"Keep me","status":"active"}]}"#,
        )));
        app.update(Msg::Event(Event::ToolCall {
            id: "t2".to_owned(),
            name: "todo".to_owned(),
            arguments: r#"{"items":[]}"#.to_owned(),
        }));
    });
    let shown = lines.join("\n");
    assert!(shown.contains("Keep me"), "{shown}");
}
