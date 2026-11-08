use std::time::{Duration, Instant};

use sigil::events::{Key, ModifiersState};

use super::harness::{Step, named, steps, typed};
use super::in_window;
use crate::pane::Pane;
use summonr_agent::hub::{Fact, Msg};

#[test]
fn the_right_panel_switches_between_changes_files_and_a_terminal() {
    if !in_window("side::the_right_panel_switches_between_changes_files_and_a_terminal") {
        return;
    }
    let open = |clicks: Vec<Step>| {
        steps(
            |app| app.toggle_pane(Pane::Diff),
            clicks,
            |app| app.layout.peek().secondary,
        )
    };
    assert_eq!(open(vec![Step::Click("Files")]).read, Some(Pane::Files));
    let terminal = open(vec![Step::Click("Terminal")]);
    assert_eq!(terminal.read, Some(Pane::Terminal));
    assert_eq!(
        open(vec![Step::Click("Terminal"), Step::Click("Changes")]).read,
        Some(Pane::Diff)
    );
    assert_eq!(open(vec![Step::Click("Close panel")]).read, None);
}

#[test]
fn the_terminal_tab_runs_commands_in_the_chats_folder() {
    if !in_window("side::the_terminal_tab_runs_commands_in_the_chats_folder") {
        return;
    }
    let runtime: &'static mut sigil::tasks::AsyncRuntime = Box::leak(Box::new(
        sigil::tasks::AsyncRuntime::new().expect("runtime"),
    ));
    runtime.install_current();
    let mut clicks = vec![Step::Click("Shell")];
    clicks.extend(typed("pwd").into_iter().map(Step::Key));
    clicks.push(Step::Key(named(Key::Enter, ModifiersState::empty())));
    let seen = steps(
        |app| app.toggle_pane(Pane::Terminal),
        clicks,
        |app| {
            let folder = std::fs::canonicalize(app.workspace())
                .expect("folder")
                .display()
                .to_string();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                runtime.pump();
                let screen = app
                    .window
                    .terminals
                    .list
                    .peek()
                    .first()
                    .map(|term| term.session.lines.get())
                    .unwrap_or_default();
                if screen.iter().any(|line| line.trim() == folder) || Instant::now() > deadline {
                    return (folder, screen);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        },
    );
    let (folder, screen) = seen.read;
    assert!(
        screen.iter().any(|line| line.trim() == folder),
        "{folder} not in {screen:?}"
    );
}

fn panel(name: &str, title: &str, every_secs: Option<u64>) -> summonr_agent::panel::Panel {
    summonr_agent::panel::Panel {
        name: name.to_string(),
        title: title.to_string(),
        kind: summonr_agent::panel::Fill::Data,
        place: summonr_agent::panel::Place::Right,
        command: vec!["true".to_string()],
        every_secs,
        root: None,
        problem: None,
    }
}

fn panel_runs(sent: &[summonr_agent::hub::Cmd], name: &str) -> usize {
    sent.iter()
        .filter(|cmd| matches!(cmd, summonr_agent::hub::Cmd::RunPanel(run) if run == name))
        .count()
}

#[test]
fn a_panel_file_becomes_a_tab_that_runs_only_while_open() {
    if !in_window("side::a_panel_file_becomes_a_tab_that_runs_only_while_open") {
        return;
    }
    let seen = steps(
        |app| {
            app.toggle_pane(Pane::Diff);
            app.update(Msg::Fact(Fact::Panels(vec![panel(
                "build",
                "Build times",
                None,
            )])));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.shows(|text| text == "Build times"), "{}", seen.text());
    assert_eq!(panel_runs(&seen.sent, "build"), 0);

    let seen = steps(
        |app| {
            app.toggle_pane(Pane::Diff);
            app.update(Msg::Fact(Fact::Panels(vec![panel(
                "build",
                "Build times",
                None,
            )])));
        },
        vec![Step::Click("Build times"), Step::Click("Files")],
        |app| app.layout.peek().secondary,
    );
    assert_eq!(seen.read, Some(Pane::Files));
    assert_eq!(panel_runs(&seen.sent, "build"), 1);
}

#[test]
fn an_open_panel_draws_its_chart_or_says_why_it_failed() {
    if !in_window("side::an_open_panel_draws_its_chart_or_says_why_it_failed") {
        return;
    }
    let open = |result: Result<summonr_agent::panel::Content, String>| {
        steps(
            |app| {
                app.toggle_pane(Pane::Diff);
                app.update(Msg::Fact(Fact::Panels(vec![panel(
                    "build",
                    "Build times",
                    None,
                )])));
            },
            vec![
                Step::Click("Build times"),
                Step::Wait(Box::new(move |app| {
                    app.update(Msg::Fact(Fact::Panel {
                        name: "build".to_string(),
                        result,
                    }));
                })),
            ],
            |_| (),
        )
    };
    let chart = open(Ok(summonr_agent::panel::Content::Chart {
        kind: summonr_agent::panel::Plot::Line,
        labels: vec!["Mon".to_string(), "Tue".to_string()],
        series: vec![summonr_agent::panel::Series {
            name: "p50".to_string(),
            values: vec![41.0, 38.5],
        }],
    }));
    assert!(chart.shows(|text| text == "Mon"), "{}", chart.text());
    let failed = open(Err("invalid output: expected value".to_string()));
    assert!(
        failed.shows(|text| text == "invalid output: expected value"),
        "{}",
        failed.text()
    );
    let stats = open(Ok(summonr_agent::panel::Content::Stat {
        items: vec![summonr_agent::panel::Stat {
            label: "p50".to_string(),
            value: "38 ms".to_string(),
            tone: summonr_agent::panel::Mood::Success,
        }],
    }));
    assert!(stats.shows(|text| text == "38 ms"), "{}", stats.text());
    let table = open(Ok(summonr_agent::panel::Content::Table {
        columns: vec!["host".to_string()],
        rows: vec![vec!["web-1".to_string()]],
    }));
    assert!(table.shows(|text| text == "host"), "{}", table.text());
    assert!(table.shows(|text| text == "web-1"), "{}", table.text());
    let words = open(Ok(summonr_agent::panel::Content::Text {
        text: "all green".to_string(),
    }));
    assert!(words.shows(|text| text == "all green"), "{}", words.text());
}

#[test]
fn a_repeating_panel_runs_again_and_stops_when_it_is_hidden() {
    if !in_window("side::a_repeating_panel_runs_again_and_stops_when_it_is_hidden") {
        return;
    }
    let run = |clicks: Vec<Step>| {
        let runtime = super::terminals::pumped();
        let mut clicks = clicks;
        clicks.push(Step::Wait(Box::new(move |_| {
            let until = Instant::now() + Duration::from_millis(1300);
            while Instant::now() < until {
                runtime.borrow_mut().pump();
                std::thread::sleep(Duration::from_millis(20));
            }
        })));
        let seen = steps(
            |app| {
                app.toggle_pane(Pane::Diff);
                app.update(Msg::Fact(Fact::Panels(vec![panel(
                    "build",
                    "Build times",
                    Some(1),
                )])));
            },
            clicks,
            |_| (),
        );
        panel_runs(&seen.sent, "build")
    };
    assert!(run(vec![Step::Click("Build times")]) >= 2);
    assert_eq!(
        run(vec![Step::Click("Build times"), Step::Click("Files")]),
        1
    );
}

#[test]
fn a_terminal_panel_opens_its_program_in_a_terminal() {
    if !in_window("side::a_terminal_panel_opens_its_program_in_a_terminal") {
        return;
    }
    let _runtime = super::terminals::pumped();
    let seen = steps(
        |app| {
            app.toggle_pane(Pane::Diff);
            app.update(Msg::Fact(Fact::Panels(vec![summonr_agent::panel::Panel {
                kind: summonr_agent::panel::Fill::Terminal,
                ..panel("review", "Review", None)
            }])));
        },
        vec![Step::Click("Review")],
        |app| app.shown_terminal().is_some(),
    );
    assert!(seen.read, "{}", seen.text());
    assert_eq!(panel_runs(&seen.sent, "review"), 0);
}
