use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

const IDS: [&str; 2] = [
    "01a104ac000000000000000000001111",
    "01a104ac000000000000000000002222",
];

fn spawn(app: &App, id: &str, assignment: &str, background: bool) {
    app.update(Msg::Event(Event::Child {
        id: id.to_owned(),
        event: Box::new(Event::Spawned {
            assignment: assignment.to_owned(),
            access: crate::child::Access::Research,
            call: "t1".to_owned(),
            background,
            workspace: String::new(),
            agent: String::new(),
        }),
    }));
}

fn act(app: &App, id: &str, event: Event) {
    app.update(Msg::Event(Event::Child {
        id: id.to_owned(),
        event: Box::new(event),
    }));
}

fn task_call(app: &App) {
    app.update(Msg::Event(Event::ToolCall {
        id: "t1".to_owned(),
        name: "task".to_owned(),
        arguments: "{}".to_owned(),
    }));
}

#[test]
fn children_the_turn_waits_on_sit_under_their_task_call() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        task_call(app);
        spawn(
            app,
            IDS[0],
            "Find duplicated helpers. Read every module.",
            false,
        );
        spawn(app, IDS[1], "Check the docs against the code.", false);
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"src/main.rs"}"#.to_owned(),
            },
        );
        act(
            app,
            IDS[1],
            Event::Assistant {
                text: "4 findings".to_owned(),
            },
        );
    })
    .join("\n");
    assert!(
        shown.contains("● Task 2 children") || shown.contains("○ Task 2 children"),
        "{shown}"
    );
    assert!(shown.contains("Find duplicated helpers"), "{shown}");
    assert!(shown.contains("Read src/main.rs"), "{shown}");
    assert!(
        shown.contains("Check the docs against the code") && shown.contains("4 findings"),
        "{shown}"
    );
    assert!(!shown.contains("background"), "{shown}");
}

#[test]
fn a_background_child_runs_in_the_strip_and_reports_in_the_transcript() {
    let mut running = String::new();
    let done = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        task_call(app);
        spawn(app, IDS[0], "Benchmark startup and RSS.", true);
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "run_command".to_owned(),
                arguments: r#"{"command":"hyperfine ./summonr"}"#.to_owned(),
            },
        );
        running = super::super::chat::panels::strip_rows(app).to_string();
        act(
            app,
            IDS[0],
            Event::Assistant {
                text: "startup 0.28 s".to_owned(),
            },
        );
    })
    .join("\n");
    assert_eq!(running, "1");
    assert!(
        done.contains("Task Benchmark startup and RSS") && done.contains("background"),
        "{done}"
    );
    assert!(
        done.contains("● Background Benchmark startup and RSS"),
        "{done}"
    );
    assert!(done.contains("startup 0.28 s"), "{done}");
}

#[test]
fn a_child_in_its_own_checkout_shows_paths_from_that_checkout() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        task_call(app);
        app.update(Msg::Event(Event::Child {
            id: IDS[0].to_owned(),
            event: Box::new(Event::Spawned {
                assignment: "Fix the parser.".to_owned(),
                access: crate::child::Access::Implement,
                call: "t1".to_owned(),
                background: false,
                workspace: "/w/repo-deltas/abc".to_owned(),
                agent: String::new(),
            }),
        }));
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"/w/repo-deltas/abc/src/lib.rs"}"#.to_owned(),
            },
        );
    })
    .join("\n");
    assert!(shown.contains("Read src/lib.rs"), "{shown}");
    assert!(!shown.contains("repo-deltas"), "{shown}");
}

#[test]
fn a_child_receipt_reads_as_its_answer_not_json() {
    use crate::child::{ChildRecord, ChildResult, ChildState};
    let record = ChildRecord {
        version: 1,
        id: IDS[0].parse().unwrap(),
        parent: IDS[1].parse().unwrap(),
        assignment: "Fix the parser. Keep the tests.".to_owned(),
        model: "m".to_owned(),
        workspace: "/w/repo-deltas/abc".into(),
        depth: 1,
        access: crate::child::Access::Implement,
        call: Some("t9".to_owned()),
        state: ChildState::Completed(ChildResult {
            text: "Parser fixed; 4 tests pass.".to_owned(),
            changed_paths: vec!["/w/repo-deltas/abc/src/parse.rs".to_owned()],
            changed_paths_total: 1,
            rounds: 3,
            usage: crate::provider::Usage::default(),
            billed_tokens: 0,
            elapsed_millis: 0,
            diagnostic: None,
            kept: None,
        }),
    };
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        app.update(Msg::Event(Event::ToolCall {
            id: "t9".to_owned(),
            name: "task".to_owned(),
            arguments: format!(r#"{{"id":"{}"}}"#, IDS[0]),
        }));
        app.update(Msg::Event(Event::ToolResult {
            id: "t9".to_owned(),
            success: true,
            mutated: false,
            output: serde_json::to_string(&record).unwrap(),
            millis: None,
            patch: None,
        }));
    })
    .join("\n");
    assert!(shown.contains("Task Fix the parser"), "{shown}");
    assert!(shown.contains("Parser fixed; 4 tests pass."), "{shown}");
    assert!(shown.contains("src/parse.rs"), "{shown}");
    assert!(
        !shown.contains("\"status\"") && !shown.contains("repo-deltas"),
        "{shown}"
    );
}

#[test]
fn a_child_that_recovers_from_a_failed_command_reads_done_and_one_that_aborts_reads_failed() {
    let shown = frame_with(busy_info(), 140, 40, State::Idle, |app| {
        task_call(app);
        spawn(app, IDS[0], "Fix the parser.", false);
        spawn(app, IDS[1], "Fix the lexer.", false);
        act(
            app,
            IDS[0],
            Event::ToolResult {
                id: "c".to_owned(),
                success: false,
                mutated: false,
                output: "1 test failed".to_owned(),
                millis: None,
                patch: None,
            },
        );
        act(
            app,
            IDS[0],
            Event::Assistant {
                text: "parser fixed".to_owned(),
            },
        );
        act(
            app,
            IDS[1],
            Event::Aborted {
                reason: "budget reached".to_owned(),
                code: crate::tools::AbortCode::TimeBudget,
            },
        );
    })
    .join("\n");
    assert!(shown.contains("1 done · 1 failed"), "{shown}");
    assert!(shown.contains("parser fixed"), "{shown}");
}
