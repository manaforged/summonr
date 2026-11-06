use super::harness::{Step, drive, steps};
use super::in_window;
use crate::tools::Event;
use crate::tui::{Fact, Msg};

const CHILD: &str = "01a104ac000000000000000000001111";

fn child_task(app: &super::App) {
    app.update(Msg::Fact(Fact::Event(Event::ToolCall {
        id: "task-call".to_owned(),
        name: "task".to_owned(),
        arguments: "{}".to_owned(),
    })));
    app.update(Msg::Fact(Fact::Event(Event::Child {
        id: CHILD.to_owned(),
        event: Box::new(Event::Spawned {
            assignment: "Check the parser".to_owned(),
            access: crate::child::Access::Research,
            call: "task-call".to_owned(),
            background: false,
            workspace: String::new(),
            agent: String::new(),
            name: String::new(),
            model: "test-model".to_string(),
        }),
    })));
}

#[test]
fn a_running_child_shows_its_assignment_in_the_desktop_transcript() {
    if !in_window("transcript::a_running_child_shows_its_assignment_in_the_desktop_transcript") {
        return;
    }
    let seen = drive(child_task, Vec::new(), |_| ());
    assert!(
        seen.shows(|text| text == "Check the parser") && seen.shows(|text| text == "Running"),
        "{}",
        seen.text()
    );
}

#[test]
fn a_mounted_child_row_shows_completion_when_the_child_finishes() {
    if !in_window("transcript::a_mounted_child_row_shows_completion_when_the_child_finishes") {
        return;
    }
    let seen = drive(child_task, Vec::new(), |app| {
        app.update(Msg::Fact(Fact::Event(Event::Child {
            id: CHILD.to_owned(),
            event: Box::new(Event::Assistant {
                text: "Parser checked".to_owned(),
            }),
        })));
    });
    assert!(
        seen.shows(|text| text == "Check the parser") && seen.shows(|text| text == "Done"),
        "{}",
        seen.text()
    );
    assert!(!seen.shows(|text| text == "Running"), "{}", seen.text());
}

#[test]
fn run_notes_stay_out_of_the_window_transcript() {
    if !in_window("transcript::run_notes_stay_out_of_the_window_transcript") {
        return;
    }
    let seen = drive(
        |app| {
            app.update(Msg::Fact(Fact::Event(Event::Assistant { text: "Ready.".to_owned() })));
            app.update(Msg::Fact(Fact::Approval("approval mode: on".to_owned())));
        },
        Vec::new(),
        |app| app.update(Msg::Fact(Fact::Approval("approval mode:off".to_owned()))),
    );
    assert!(seen.shows(|text| text == "Ready."), "{}", seen.text());
    assert!(
        !seen.shows(|text| text.starts_with("approval mode")),
        "{}",
        seen.text()
    );
}

fn tool(app: &super::App, id: &str, name: &str, arguments: &str) {
    app.update(Msg::Fact(Fact::Event(Event::ToolCall {
        id: id.to_owned(),
        name: name.to_owned(),
        arguments: arguments.to_owned(),
    })));
    app.update(Msg::Fact(Fact::Event(Event::ToolResult {
        id: id.to_owned(),
        success: true,
        mutated: name == "write",
        output: "ok".to_owned(),
        millis: Some(4),
        patch: None,
    })));
}

#[test]
fn a_finished_reply_says_what_it_did_in_words() {
    if !in_window("transcript::a_finished_reply_says_what_it_did_in_words") {
        return;
    }
    let seen = drive(
        |app| {
            tool(app, "a", "write", r#"{"path":"src/a.rs","content":"x"}"#);
            tool(app, "b", "write", r#"{"path":"src/b.rs","content":"y"}"#);
            tool(app, "c", "run_command", r#"{"command":"cargo test"}"#);
            tool(app, "d", "read", r#"{"path":"README.md"}"#);
            app.update(Msg::Fact(Fact::Event(Event::Assistant { text: "Done.".to_owned() })));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "Edited 2 files, ran 1 command, looked at 1 file"),
        "{}",
        seen.text()
    );
}

fn ran_tests(app: &super::App) {
    tool(app, "c", "run_command", r#"{"command":"cargo test --workspace"}"#);
    app.update(Msg::Fact(Fact::Event(Event::Assistant { text: "Done.".to_owned() })));
}

#[test]
fn all_steps_in_the_palette_unfolds_finished_replies_and_folds_them_again() {
    if !in_window("transcript::all_steps_in_the_palette_unfolds_finished_replies_and_folds_them_again") {
        return;
    }
    let folded = drive(ran_tests, Vec::new(), |_| ());
    assert!(!folded.shows(|text| text.contains("cargo test --workspace")), "{}", folded.text());
    let every = steps(ran_tests, vec![Step::Click("Search"), Step::Click("All steps")], |_| ());
    assert!(every.shows(|text| text.contains("cargo test --workspace")), "{}", every.text());
    let back = steps(
        ran_tests,
        vec![Step::Click("Search"), Step::Click("All steps"), Step::Click("Search"), Step::Click("All steps")],
        |_| (),
    );
    assert!(!back.shows(|text| text.contains("cargo test --workspace")), "{}", back.text());
}
