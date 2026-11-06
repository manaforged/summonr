use std::time::{Duration, Instant};

use sigil::events::{Key, ModifiersState};

use super::harness::{Step, named, steps, typed};
use super::in_window;
use crate::pane::Pane;

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
