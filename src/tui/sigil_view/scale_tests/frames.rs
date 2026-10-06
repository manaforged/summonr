use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::{App, Login, Note};
use super::modals::{rejected_openai, select};
use super::{State, busy_info, frame_with};

fn screen(cols: u16, rows: u16, setup: impl FnOnce(&App)) -> String {
    frame_with(busy_info(), cols, rows, State::Idle, setup).join("\n")
}

fn intact(screen: &str) -> bool {
    let lines: Vec<&str> = screen.lines().collect();
    let (Some(top), Some(bottom)) = (
        lines
            .iter()
            .position(|line| line.contains('┏') || line.contains('╭')),
        lines
            .iter()
            .position(|line| line.contains('┗') || line.contains('╰')),
    ) else {
        return false;
    };
    let edge = |line: &str| line.trim_end_matches('█').chars().last();
    let right = edge(lines[top]).map(|corner| if corner == '┓' { '┃' } else { '│' });
    lines[top + 1..bottom]
        .iter()
        .all(|line| edge(line) == right)
}

fn login(app: &App) {
    app.replace("/login".to_owned());
    app.submit();
}

#[test]
fn a_login_step_from_a_narrow_model_picker_stays_inside_its_frame() {
    let screen = screen(100, 30, |app| {
        rejected_openai(app);
        select(app, "openai");
        app.commit_model_edit();
    });
    assert!(screen.contains("openai"), "{screen}");
    assert!(screen.contains("esc back"), "{screen}");
    assert!(intact(&screen), "{screen}");
}

#[test]
fn a_narrow_login_note_shows_its_second_line() {
    let screen = screen(90, 30, |app| {
        login(app);
        app.auth.step.set(Login::List(Note::plain(
            "openai rejected the key that the shell exports, so paste a new key or sign in with a plan instead"
                .to_owned(),
        )));
    });
    assert!(screen.contains("instead"), "{screen}");
    assert!(intact(&screen), "{screen}");
}

fn guarded(act: impl FnOnce(&App) -> bool) -> bool {
    Runtime::scope("summonr-frames", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(160, 40);
        act(&app)
    })
}

#[test]
fn the_provider_cannot_change_during_a_login_step() {
    assert!(guarded(|app| {
        login(app);
        let held = app.auth.sel.peek();
        app.auth.step.set(Login::Confirm);
        app.replace_auth_filter("deep".to_owned());
        app.pick_auth(held + 1);
        app.auth.sel.peek() == held && app.auth.filter.peek().is_empty()
    }));
}

#[test]
fn a_model_click_during_a_login_step_keeps_the_picker_open() {
    assert!(guarded(|app| {
        rejected_openai(app);
        select(app, "openai");
        app.commit_model_edit();
        let stacked = app.auth.open.peek();
        select(app, "deepseek");
        app.commit_model_edit();
        stacked && app.picker.open.peek()
    }));
}

#[test]
fn arrows_do_not_move_a_hidden_provider_list() {
    assert!(guarded(|app| {
        rejected_openai(app);
        select(app, "openai");
        app.commit_model_edit();
        let held = app.auth.sel.peek();
        app.move_auth(1);
        app.auth.sel.peek() == held
    }));
}
