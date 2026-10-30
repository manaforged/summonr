use super::harness::drive;
use super::in_window;
use crate::tools::Event;
use crate::tui::Msg;

const CHILD: &str = "01a104ac000000000000000000001111";

fn child_task(app: &super::App) {
    app.update(Msg::Event(Event::ToolCall {
        id: "task-call".to_owned(),
        name: "task".to_owned(),
        arguments: "{}".to_owned(),
    }));
    app.update(Msg::Event(Event::Child {
        id: CHILD.to_owned(),
        event: Box::new(Event::Spawned {
            assignment: "Check the parser".to_owned(),
            access: crate::child::Access::Research,
            call: "task-call".to_owned(),
            background: false,
            workspace: String::new(),
            agent: String::new(),
        }),
    }));
}

#[test]
fn a_running_child_shows_its_assignment_in_the_desktop_transcript() {
    if !in_window("transcript::a_running_child_shows_its_assignment_in_the_desktop_transcript") {
        return;
    }
    let seen = drive(child_task, Vec::new(), |_| ());
    assert!(
        seen.shows(|text| text == "running: “Check the parser”"),
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
        app.update(Msg::Event(Event::Child {
            id: CHILD.to_owned(),
            event: Box::new(Event::Assistant {
                text: "Parser checked".to_owned(),
            }),
        }));
    });
    assert!(
        seen.shows(|text| text.starts_with("completed in ") && text.ends_with("“Check the parser”")),
        "{}",
        seen.text()
    );
    assert!(!seen.shows(|text| text.starts_with("running: “Check the parser”")));
}

#[test]
fn approval_replacement_updates_the_mounted_transcript() {
    if !in_window("transcript::approval_replacement_updates_the_mounted_transcript") {
        return;
    }
    let seen = drive(
        |app| app.update(Msg::Approval("approval mode: on".to_owned())),
        Vec::new(),
        |app| app.update(Msg::Approval("approval mode:off".to_owned())),
    );
    assert!(
        seen.shows(|text| text == "approval mode:off"),
        "{}",
        seen.text()
    );
    assert!(
        !seen.shows(|text| text == "approval mode: on"),
        "{}",
        seen.text()
    );
}
