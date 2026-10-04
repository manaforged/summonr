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
