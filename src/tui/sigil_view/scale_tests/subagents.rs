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
