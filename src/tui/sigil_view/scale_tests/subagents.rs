use super::super::app::App;
use super::children::{IDS, act, spawn, task_call};
use super::{State, busy_info, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

#[test]
fn a_running_subagent_reads_as_its_task_not_its_status_dump() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        task_call(app);
        app.update(Msg::Event(Event::Child {
            id: IDS[0].to_owned(),
            event: Box::new(Event::Spawned {
                assignment: "Fix total() so it rounds to cents.".to_owned(),
                access: crate::child::Access::Implement,
                call: "t1".to_owned(),
                background: true,
                workspace: "/w/repo-children/abc".to_owned(),
                agent: String::new(),
            }),
        }));
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"/w/repo-children/abc/cart.py"}"#.to_owned(),
            },
        );
        app.update(Msg::Event(Event::ToolCall {
            id: "p1".to_owned(),
            name: "task".to_owned(),
            arguments: format!(r#"{{"child":"{}"}}"#, IDS[0]),
        }));
        app.update(Msg::Event(Event::ToolResult {
            id: "p1".to_owned(),
            success: true,
            mutated: false,
            output: format!(
                "child {}\nstate: running\nelapsed: 3s\nassignment: Fix total()\nlast: read {{\"path\":\"/w/repo-children/abc/cart.py\"}}",
                IDS[0]
            ),
            millis: None,
            patch: None,
        }));
    })
    .join("\n");
    assert!(!shown.contains(IDS[0]), "no raw child id: {shown}");
    assert!(!shown.contains("assignment:"), "no status dump: {shown}");
    assert!(shown.contains("Read cart.py"), "{shown}");
    assert!(!shown.contains("repo-children"), "{shown}");
    assert!(
        !shown.contains("subagent "),
        "no repeated lead word: {shown}"
    );
    assert!(
        shown.matches("Fix total() so it rounds to cents").count() >= 2,
        "the poll and the strip both name the task: {shown}"
    );
}

#[test]
fn subagents_read_as_one_line_each_that_flips_when_done() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        task_call(app);
        spawn(app, IDS[0], "Round total to cents. Keep the tests.", true);
        spawn(app, IDS[1], "Add a --dry-run flag to cli.py.", true);
        act(
            app,
            IDS[1],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"cli.py"}"#.to_owned(),
            },
        );
        act(
            app,
            IDS[0],
            Event::Assistant {
                text: "total() rounds to cents".to_owned(),
            },
        );
        app.update(Msg::Event(Event::ToolCall {
            id: "p1".to_owned(),
            name: "task".to_owned(),
            arguments: format!(r#"{{"child":"{}"}}"#, IDS[1]),
        }));
    })
    .join("\n");
    assert!(
        shown.contains("◆ Subagent completed in") && shown.contains("“Round total to cents”"),
        "{shown}"
    );
    assert!(
        shown.contains("◆ Subagent running: “Add a --dry-run flag to cli.py”"),
        "{shown}"
    );
    assert!(
        shown.contains("Subagents 1"),
        "the panel counts running subagents: {shown}"
    );
    assert!(
        !shown.contains("Task"),
        "polls and spawns draw no Task card: {shown}"
    );
}

#[test]
fn subagents_spawned_by_separate_calls_each_get_their_own_line() {
    let assignments = [
        "Fix total() in pricing.py to round to cents (two decimals). Keep it scoped.",
        "Add --dry-run flag to cli.py that prints what it would charge, clearly marked.",
        "Document CLI in README.md: positional prices, --tax, and --dry-run.",
    ];
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        for (at, (id, assignment)) in IDS.iter().zip(assignments).enumerate() {
            let call = format!("s{at}");
            app.update(Msg::Event(Event::ToolCall {
                id: call.clone(),
                name: "task".to_owned(),
                arguments: r#"{"background":true}"#.to_owned(),
            }));
            app.update(Msg::Event(Event::Child {
                id: (*id).to_owned(),
                event: Box::new(Event::Spawned {
                    assignment: assignment.to_owned(),
                    access: crate::child::Access::Implement,
                    call: call.clone(),
                    background: true,
                    workspace: String::new(),
                    agent: String::new(),
                }),
            }));
            app.update(Msg::Event(Event::ToolResult {
                id: call,
                success: true,
                mutated: false,
                output: format!("started child {id}\nstate: running"),
                millis: None,
                patch: None,
            }));
        }
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"pricing.py"}"#.to_owned(),
            },
        );
    })
    .join("\n");
    assert_eq!(shown.matches("◆ Subagent running").count(), 3, "{shown}");
    assert!(!shown.contains("started child"), "{shown}");
    assert!(
        shown.contains("“Fix total() in pricing.py to round to cents”"),
        "{shown}"
    );
    assert!(shown.contains("“Document CLI in README.md”"), "{shown}");
    assert!(
        shown.contains("Read pricing.py"),
        "the panel keeps room for the action: {shown}"
    );
}

#[test]
fn parallel_spawn_calls_each_get_their_own_subagent_line() {
    let assignments = [
        "Fix total() in pricing.py.",
        "Add --dry-run to cli.py.",
        "Document the CLI in README.md.",
    ];
    let calls = ["call_a", "call_b", "call_c"];
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        for call in calls {
            app.update(Msg::Event(Event::ToolCall {
                id: call.to_owned(),
                name: "task".to_owned(),
                arguments: r#"{"background":true}"#.to_owned(),
            }));
        }
        for ((id, assignment), call) in IDS.iter().zip(assignments).zip(calls) {
            app.update(Msg::Event(Event::Child {
                id: (*id).to_owned(),
                event: Box::new(Event::Spawned {
                    assignment: assignment.to_owned(),
                    access: crate::child::Access::Implement,
                    call: call.to_owned(),
                    background: true,
                    workspace: String::new(),
                    agent: String::new(),
                }),
            }));
        }
        for (id, call) in IDS.iter().zip(calls) {
            app.update(Msg::Event(Event::ToolResult {
                id: call.to_owned(),
                success: true,
                mutated: false,
                output: format!("started child {id}\nstate: running"),
                millis: None,
                patch: None,
            }));
        }
    })
    .join("\n");
    assert_eq!(shown.matches("◆ Subagent running").count(), 3, "{shown}");
    assert!(!shown.contains("started child"), "{shown}");
}

#[test]
fn a_spawn_drawn_before_its_subagent_exists_redraws_as_the_subagent_line() {
    use super::super::view::root;
    use sigil::{Host, events::EventRouter, reactive::Runtime, terminal::Screen};
    use tokio::sync::mpsc::unbounded_channel;

    Runtime::scope("spawn-redraw", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(120, 30);
        app.update(Msg::Busy(Some(crate::tui::Activity::Turn)));
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(120, 30);
        let mut router = EventRouter::new();
        let mut frame = |screen: &mut Screen| {
            screen
                .frame(&mut host, &mut router, [], (120, 30))
                .expect("frame")
                .text()
        };
        for call in ["call_a", "call_b"] {
            app.update(Msg::Event(Event::ToolCall {
                id: call.to_owned(),
                name: "task".to_owned(),
                arguments: r#"{"background":true}"#.to_owned(),
            }));
        }
        frame(&mut screen);
        for (id, call) in IDS.iter().zip(["call_a", "call_b"]) {
            spawn_on(&app, id, call, "Round total to cents.");
            app.update(Msg::Event(Event::ToolResult {
                id: call.to_owned(),
                success: true,
                mutated: false,
                output: format!("started child {id}\nstate: running"),
                millis: None,
                patch: None,
            }));
        }
        frame(&mut screen);
        act(
            &app,
            IDS[0],
            Event::Assistant {
                text: "done".to_owned(),
            },
        );
        let shown = frame(&mut screen);
        assert!(!shown.contains("started child"), "{shown}");
        assert!(
            shown.contains("◆ Subagent completed in"),
            "the line follows the child: {shown}"
        );
        assert!(shown.contains("◆ Subagent running"), "{shown}");
    });
}

fn spawn_on(app: &App, id: &str, call: &str, assignment: &str) {
    app.update(Msg::Event(Event::Child {
        id: id.to_owned(),
        event: Box::new(Event::Spawned {
            assignment: assignment.to_owned(),
            access: crate::child::Access::Implement,
            call: call.to_owned(),
            background: true,
            workspace: String::new(),
            agent: String::new(),
        }),
    }));
}

#[test]
fn consecutive_subagent_lines_sit_on_consecutive_rows() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        for (id, call) in IDS.iter().zip(["call_a", "call_b", "call_c"]) {
            app.update(Msg::Event(Event::ToolCall {
                id: call.to_owned(),
                name: "task".to_owned(),
                arguments: r#"{"background":true}"#.to_owned(),
            }));
            spawn_on(app, id, call, "Round total to cents.");
        }
    });
    let rows: Vec<usize> = shown
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("◆ Subagent"))
        .map(|(at, _)| at)
        .collect();
    assert_eq!(rows.len(), 3, "{}", shown.join("\n"));
    assert!(
        rows.windows(2).all(|pair| pair[1] == pair[0] + 1),
        "{}",
        shown.join("\n")
    );
}

#[test]
fn a_spawn_result_before_its_subagent_shows_a_starting_line_not_raw_output() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        app.update(Msg::Event(Event::ToolCall {
            id: "call_a".to_owned(),
            name: "task".to_owned(),
            arguments: r#"{"assignment":"Round total to cents.","background":true}"#.to_owned(),
        }));
        app.update(Msg::Event(Event::ToolResult {
            id: "call_a".to_owned(),
            success: true,
            mutated: false,
            output: format!("started child {}\nstate: running\npoll with task", IDS[0]),
            millis: None,
            patch: None,
        }));
    })
    .join("\n");
    assert!(!shown.contains("started child"), "{shown}");
    assert!(shown.contains("◆ Subagent starting"), "{shown}");
}

#[test]
fn a_narrow_subagent_row_keeps_its_activity_and_names_a_listing_as_a_directory() {
    let shown = frame_with(busy_info(), 84, 25, State::Busy, |app| {
        task_call(app);
        spawn(
            app,
            IDS[0],
            "Add --dry-run flag to cli.py that prints what it would charge.",
            true,
        );
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":".","op":"list"}"#.to_owned(),
            },
        );
    });
    let rows: Vec<&String> = shown
        .iter()
        .filter(|line| line.trim_start().starts_with("○ Subagent"))
        .collect();
    assert!(!rows.is_empty(), "{}", shown.join("\n"));
    for row in rows {
        assert!(row.contains("· Read ./"), "{row}");
        assert!(!row.contains("· …"), "{row}");
    }
    assert!(!shown.join("\n").contains(". list"), "{}", shown.join("\n"));
}
