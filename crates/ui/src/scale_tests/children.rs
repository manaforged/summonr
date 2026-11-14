use super::super::app::App;
use super::{State, busy_info, frame_with, screens};
use summonr_agent::hub::{Fact, Msg};
use summonr_agent::tools::Event;

pub(super) const IDS: [&str; 3] = [
    "01a104ac000000000000000000001111",
    "01a104ac000000000000000000002222",
    "01a104ac000000000000000000003333",
];

pub(super) fn spawn(app: &App, id: &str, assignment: &str, background: bool) {
    app.update(Msg::Fact(Fact::Event(Event::Child {
        id: id.to_owned(),
        event: Box::new(Event::Spawned {
            assignment: assignment.to_owned(),
            access: summonr_agent::child::Access::Research,
            call: "t1".to_owned(),
            background,
            workspace: String::new(),
            agent: String::new(),
            name: String::new(),
            model: "test-model".to_string(),
        }),
    })));
}

pub(super) fn act(app: &App, id: &str, event: Event) {
    app.update(Msg::Fact(Fact::Event(Event::Child {
        id: id.to_owned(),
        event: Box::new(event),
    })));
}

pub(super) fn task_call(app: &App) {
    app.update(Msg::Fact(Fact::Event(Event::ToolCall {
        id: "t1".to_owned(),
        name: "task".to_owned(),
        arguments: "{}".to_owned(),
    })));
}

#[test]
fn goal_briefs_show_the_task_and_web_host_without_repeated_labels() {
    let shown = frame_with(busy_info(), 164, 40, State::Busy, |app| {
        task_call(app);
        spawn(app, IDS[0], "GOAL: Check subscription policy. PATHS: read-only", true);
        act(app, IDS[0], Event::ToolCall {
            id: "web-policy".to_string(),
            name: "web".to_string(),
            arguments: serde_json::json!({"url": "https://code.claude.com/docs/en/legal-and-compliance"}).to_string(),
        });
    }).join("\n");
    assert!(shown.contains("Check subscription policy"), "{shown}");
    assert!(shown.contains("Web code.claude.com"), "{shown}");
    assert!(!shown.contains("GOAL"), "{shown}");
    assert!(!shown.contains("/docs/en/legal-and-compliance"), "{shown}");
    assert!(!shown.contains("Subagent Check subscription"), "{shown}");
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
        shown.contains("✳ Subagent running: “Find duplicated helpers”"),
        "{shown}"
    );
    assert!(shown.contains("Read src/main.rs"), "{shown}");
    assert!(
        shown.contains("◆ Subagent completed in")
            && shown.contains("“Check the docs against the code”"),
        "{shown}"
    );
    assert!(!shown.contains("subagent"), "{shown}");
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
        running = super::super::chat::panels::status_rows(app).to_string();
        act(
            app,
            IDS[0],
            Event::Assistant {
                text: "startup 0.28 s".to_owned(),
            },
        );
    })
    .join("\n");
    assert_eq!(running, "1", "background activity uses one status row");
    assert!(
        done.contains("◆ Subagent completed in") && done.contains("“Benchmark startup and RSS”"),
        "{done}"
    );
}

#[test]
fn a_child_in_its_own_checkout_shows_paths_from_that_checkout() {
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        task_call(app);
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: IDS[0].to_owned(),
            event: Box::new(Event::Spawned {
                assignment: "Fix the parser.".to_owned(),
                access: summonr_agent::child::Access::Implement,
                call: "t1".to_owned(),
                background: false,
                workspace: "/w/repo-children/abc".to_owned(),
                agent: String::new(),
                name: String::new(),
                model: "test-model".to_string(),
            }),
        })));
        act(
            app,
            IDS[0],
            Event::ToolCall {
                id: "c".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"/w/repo-children/abc/src/lib.rs"}"#.to_owned(),
            },
        );
    })
    .join("\n");
    assert!(shown.contains("Read src/lib.rs"), "{shown}");
    assert!(!shown.contains("repo-children"), "{shown}");
}

#[test]
fn a_child_receipt_reads_as_its_answer_not_json() {
    use summonr_agent::child::{ChildRecord, ChildResult, ChildState};
    let record = ChildRecord {
        version: 1,
        id: IDS[0].parse().unwrap(),
        parent: IDS[1].parse().unwrap(),
        assignment: "Fix the parser. Keep the tests.".to_owned(),
        model: "m".to_owned(),
        name: String::new(),
        agent: String::new(),
        workspace: "/w/repo-children/abc".into(),
        depth: 1,
        access: summonr_agent::child::Access::Implement,
        background: false,
        call: Some("t9".to_owned()),
        base: String::new(),
        state: ChildState::Completed(ChildResult {
            text: "Parser fixed; 4 tests pass.".to_owned(),
            changed_paths: vec!["/w/repo-children/abc/src/parse.rs".to_owned()],
            changed_paths_total: 1,
            rounds: 3,
            usage: summonr_agent::provider::Usage::default(),
            billed_tokens: 0,
            elapsed_millis: 0,
            diagnostic: None,
            kept: None,
            head: String::new(),
        }),
    };
    let shown = frame_with(busy_info(), 140, 40, State::Busy, |app| {
        app.update(Msg::Fact(Fact::Event(Event::ToolCall {
            id: "t9".to_owned(),
            name: "task".to_owned(),
            arguments: format!(r#"{{"id":"{}"}}"#, IDS[0]),
        })));
        app.update(Msg::Fact(Fact::Event(Event::ToolResult {
            id: "t9".to_owned(),
            success: true,
            mutated: false,
            output: serde_json::to_string(&record).unwrap(),
            millis: None,
            patch: None,
        })));
    })
    .join("\n");
    assert!(shown.contains("Subagent Fix the parser"), "{shown}");
    assert!(shown.contains("Parser fixed; 4 tests pass."), "{shown}");
    assert!(shown.contains("src/parse.rs"), "{shown}");
    assert!(
        !shown.contains("\"status\"") && !shown.contains("repo-children"),
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
                code: summonr_agent::tools::AbortCode::TimeBudget,
            },
        );
    })
    .join("\n");
    assert!(
        shown.contains("◆ Subagent completed in") && shown.contains("“Fix the parser”"),
        "{shown}"
    );
    assert!(
        shown.contains("◆ Subagent failed after") && shown.contains("“Fix the lexer”"),
        "{shown}"
    );
}

#[test]
fn clicking_the_strip_header_expands_it() {
    use super::super::view::root;
    use sigil::{Host, terminal::Screen};

    sigil::reactive::Owner::scope("strip-header", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 30);
        spawn(&app, IDS[0], "Keep the parent working", true);
        let mut host = Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        for _ in 0..2 {
            screen.frame(&mut host, [], (100, 30)).expect("frame");
        }
        let shown = screen
            .frame(&mut host, [], (100, 30))
            .expect("frame")
            .text();
        let (row, line) = shown
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("1 agent"))
            .expect("collapsed header");
        let at = line.find("1 agent").expect("agent details button");
        let col = sigil::app::cell_width(&line[..at]);
        let point = sigil::terminal::cell_point(col as u16, row as u16);
        screen
            .frame(
                &mut host,
                [sigil::events::Event::pointer_down(point)],
                (100, 30),
            )
            .expect("press header");
        app.ticks.set(app.ticks.peek() + 1);
        screen
            .frame(&mut host, [], (100, 30))
            .expect("spinner tick while pressed");
        screen
            .frame(
                &mut host,
                [sigil::events::Event::pointer_up(point)],
                (100, 30),
            )
            .expect("release header");
        let expanded = screen
            .frame(&mut host, [], (100, 30))
            .expect("frame")
            .text();
        assert!(
            expanded.contains("Status") && expanded.contains("Agents"),
            "header click did not expand:\n{expanded}"
        );
        assert!(app.model.get().status_view.open());
        let ctrl_x = sigil::terminal::map_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::CONTROL,
        ))
        .expect("ctrl+x");
        let a = sigil::terminal::map_key(&crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        ))
        .expect("a");
        screen
            .frame(&mut host, [ctrl_x, a], (100, 30))
            .expect("chord");
        let collapsed = screen
            .frame(&mut host, [], (100, 30))
            .expect("frame")
            .text();
        assert!(
            collapsed.contains("1 agent") && !collapsed.contains("Agents"),
            "ctrl+x a did not collapse:\n{collapsed}"
        );
        assert!(!app.model.get().status_view.open());
    });
}

#[test]
fn the_status_bar_counts_every_child_in_one_row() {
    use super::super::view::root;
    use sigil::{Host, terminal::Screen};

    sigil::reactive::Owner::scope("strip-cap", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 40);
        for index in 0..8 {
            spawn(
                &app,
                &format!("01a104ac00000000000000000000{index:04x}"),
                &format!("lane {index}"),
                true,
            );
        }
        let mut host = Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 40);
        let shown = screen
            .frame(&mut host, [], (100, 40))
            .expect("frame")
            .text();
        assert!(shown.contains("8 agents"), "{shown}");
        assert!(!shown.contains("lane 0"), "{shown}");
        assert!(!shown.contains("lane 7"), "{shown}");
    });
}

#[test]
fn the_status_popup_expands_the_selected_child() {
    use super::super::view::root;
    use sigil::terminal::Screen;

    sigil::reactive::Owner::scope("strip-split", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(120, 40);
        let id = "01a104ac000000000000000000000001";
        spawn(&app, id, "lane 0", true);
        act(
            &app,
            id,
            Event::Note {
                text: "These words must wrap without losing the final sentinel".to_string(),
            },
        );
        spawn(
            &app,
            IDS[1],
            "Inspect a long task title that must remain on one row when the popup narrows",
            true,
        );
        app.toggle_agents();
        app.expand_status();
        let mut host = sigil::Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(120, 40);
        for size in [(120, 40), (60, 30), (100, 40)] {
            app.resize(size.0, size.1);
            let mut shown = String::new();
            for _ in 0..3 {
                shown = screen.frame(&mut host, [], size).expect("frame").text();
            }
            assert!(shown.contains("Status"), "{shown}");
            assert!(!shown.contains("selected"), "{shown}");
            assert!(shown.contains("lane 0"), "{shown}");
            assert!(shown.contains("test-model"), "model missing: {shown}");
            assert!(
                shown.contains("sentinel"),
                "detail clipped after resize: {shown}"
            );
        }
    });
}

#[test]
fn a_cleared_background_child_stays_hidden_without_cancelling_the_parent() {
    use super::super::{chat::panels, view::root};
    use sigil::{Host, terminal::Screen};
    use summonr_agent::hub::{Activity, Cmd};
    use tokio::sync::mpsc::{error::TryRecvError, unbounded_channel};

    sigil::reactive::Owner::scope("clear-background", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        app.resize(100, 30);
        app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
        spawn(&app, IDS[0], "Keep the parent working", true);
        spawn(&app, IDS[1], "Foreground child", false);
        app.toggle_agents();
        app.expand_status();
        let mut host = Host::new(sigil::prelude::HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        for _ in 0..2 {
            screen.frame(&mut host, [], (100, 30)).expect("frame");
        }
        let shown = screen
            .frame(&mut host, [], (100, 30))
            .expect("frame")
            .text();
        let (row, line) = shown
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("Clear"))
            .expect("Clear button");
        let col = sigil::app::cell_width(&line[..line.find("Clear").expect("Clear label")]);
        screen
            .frame(
                &mut host,
                sigil::events::Event::click(sigil::terminal::cell_point(col as u16, row as u16)),
                (100, 30),
            )
            .expect("click Clear");
        assert!(
            matches!(sent.try_recv(), Err(TryRecvError::Empty)),
            "Clear sent a worker command"
        );
        let model = app.model.get();
        assert!(model.busy_since.is_some());
        assert_eq!(model.agents.len(), 2);
        assert!(!model.agents[0].done);
        drop(model);
        app.toggle_agents();
        assert_eq!(panels::status_rows(&app), 1);
        act(
            &app,
            IDS[0],
            Event::Usage {
                prompt_tokens: 5,
                completion_tokens: 2,
                reasoning_tokens: 0,
                cached_tokens: 0,
                cache_write_tokens: 0,
                millis: None,
                prefix_hash: String::new(),
                cost: Some(0.25),
                side: false,
            },
        );
        act(
            &app,
            IDS[0],
            Event::Note {
                text: "late progress".to_owned(),
            },
        );
        assert_eq!(panels::status_rows(&app), 1);
        app.toggle_agents();
        assert_eq!(panels::status_rows(&app), 1);
        app.clear_agent(&summonr_agent::child::tag(IDS[1]));
        assert_eq!(panels::status_rows(&app), 1);
        spawn(&app, IDS[2], "New background child", true);
        assert_eq!(panels::status_rows(&app), 1);
        act(
            &app,
            IDS[0],
            Event::Assistant {
                text: "still finishes".to_owned(),
            },
        );
        let model = app.model.get();
        assert_eq!(model.agents[0].tokens, 7);
        assert_eq!(model.agents[0].cost, 0.25);
        assert!(model.agents[0].done);
        assert!(model.busy_since.is_some());
        assert_eq!(model.agents[0].last, "still finishes");
    });
}

fn read(app: &App, id: &str) {
    app.update(Msg::Fact(Fact::Event(Event::ToolCall {
        id: id.to_owned(),
        name: "read".to_owned(),
        arguments: r#"{"path":"a.txt"}"#.to_owned(),
    })));
}

fn done(app: &App, id: &str, output: &str) {
    app.update(Msg::Fact(Fact::Event(Event::ToolResult {
        id: id.to_owned(),
        success: true,
        mutated: false,
        output: output.to_owned(),
        millis: Some(3),
        patch: None,
    })));
}

#[test]
fn a_group_row_repaints_when_a_later_call_finishes_empty() {
    let setup = |app: &App| {
        read(app, "1");
        done(app, "1", "one");
        read(app, "2");
    };
    let live = screens(110, 30, setup, &[&|app| done(app, "2", "")]);
    let fresh = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        setup(app);
        done(app, "2", "");
    })
    .join("\n");
    let live = live.last().map_or("", |screen| screen.trim_end());
    assert!(
        fresh.contains("2 reads") || fresh.contains("Read"),
        "{fresh}"
    );
    assert_eq!(live, fresh.trim_end());
}
