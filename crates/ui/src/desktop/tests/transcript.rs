use super::harness::{Step, drive, steps, typed};
use super::in_window;
use summonr_agent::hub::{Cmd, Fact, Msg};
use summonr_agent::tools::Event;

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
            access: summonr_agent::child::Access::Research,
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
            app.update(Msg::Fact(Fact::Event(Event::Assistant {
                text: "Ready.".to_owned(),
            })));
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
            app.update(Msg::Fact(Fact::Event(Event::Assistant {
                text: "Done.".to_owned(),
            })));
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
    tool(
        app,
        "c",
        "run_command",
        r#"{"command":"cargo test --workspace"}"#,
    );
    app.update(Msg::Fact(Fact::Event(Event::Assistant {
        text: "Done.".to_owned(),
    })));
}

#[test]
fn all_steps_in_the_palette_unfolds_finished_replies_and_folds_them_again() {
    if !in_window(
        "transcript::all_steps_in_the_palette_unfolds_finished_replies_and_folds_them_again",
    ) {
        return;
    }
    let folded = drive(ran_tests, Vec::new(), |_| ());
    assert!(
        !folded.shows(|text| text.contains("cargo test --workspace")),
        "{}",
        folded.text()
    );
    let every = steps(ran_tests, all_steps(1), |_| ());
    assert!(
        every.shows(|text| text.contains("cargo test --workspace")),
        "{}",
        every.text()
    );
    let back = steps(ran_tests, all_steps(2), |_| ());
    assert!(
        !back.shows(|text| text.contains("cargo test --workspace")),
        "{}",
        back.text()
    );
}

fn all_steps(times: usize) -> Vec<Step> {
    let mut actions = Vec::new();
    for _ in 0..times {
        actions.push(Step::Click("Search"));
        actions.extend(typed("all st").into_iter().map(Step::Key));
        actions.push(Step::Click("All steps"));
    }
    actions
}

#[test]
fn a_failed_turn_shows_one_line_and_a_retry_button() {
    if !in_window("transcript::a_failed_turn_shows_one_line_and_a_retry_button") {
        return;
    }
    let failure = format!(
        "provider error: rate limited\nraw body{}",
        summonr_agent::hub::RETRY_HINT
    );
    let seen = steps(
        move |app| app.update(Msg::Fact(Fact::Failed(failure.clone()))),
        vec![Step::Click("Retry")],
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "provider error: rate limited"),
        "{}",
        seen.text()
    );
    assert!(
        !seen.shows(|text| text.contains("raw body")),
        "{}",
        seen.text()
    );
    assert!(!seen.shows(|text| text.contains("F5")), "{}", seen.text());
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Command(text) if text == "/retry"))
    );
}

fn thought(app: &super::App, text: &str) {
    app.update(Msg::Fact(Fact::Event(Event::Reasoning {
        text: text.to_owned(),
    })));
}

fn worked_turn(app: &super::App) {
    thought(app, "Look at the tests first.");
    tool(app, "a", "run_command", r#"{"command":"cargo test"}"#);
    thought(app, "One failure in the parser.");
    tool(app, "b", "run_command", r#"{"command":"rg parse"}"#);
    thought(app, "Fix the off-by-one.");
    tool(
        app,
        "c",
        "write",
        r#"{"path":"src/parse.rs","content":"x"}"#,
    );
    app.update(Msg::Fact(Fact::Event(Event::Assistant {
        text: "Fixed.".to_owned(),
    })));
}

#[test]
fn a_finished_turn_folds_its_thoughts_and_steps_into_one_row() {
    if !in_window("transcript::a_finished_turn_folds_its_thoughts_and_steps_into_one_row") {
        return;
    }
    let folded = drive(worked_turn, Vec::new(), |_| ());
    let rows = folded
        .texts
        .iter()
        .filter(|text| text.starts_with("Thought"))
        .count();
    assert_eq!(rows, 0, "{}", folded.text());
    assert!(
        folded.shows(|text| text == "Edited 1 file, ran 2 commands"),
        "{}",
        folded.text()
    );
    let open = steps(
        worked_turn,
        vec![Step::Click("Edited 1 file, ran 2 commands")],
        |_| (),
    );
    let thoughts = open
        .texts
        .iter()
        .filter(|text| text.starts_with("Thought"))
        .count();
    assert_eq!(thoughts, 3, "{}", open.text());
    assert!(
        open.shows(|text| text.contains("cargo test")),
        "{}",
        open.text()
    );
}
