use super::super::app::App;
use super::{busy_info, screens};
use crate::view::root;
use sigil::Host;
use sigil::prelude::HostMode;
use sigil::terminal::Screen;
use summonr_agent::hub::{Fact, Intent, Msg};
use summonr_agent::tools::Event;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn mounted_terminal_prose_streams_and_replaces_the_final_answer() {
    let shown = screens(
        100,
        30,
        |app| {
            app.update(Msg::Fact(Fact::Event(Event::Delta {
                text: "First word".to_owned(),
            })));
        },
        &[
            &|app: &App| {
                app.update(Msg::Fact(Fact::Event(Event::Delta {
                    text: " second word".to_owned(),
                })));
            },
            &|app: &App| {
                app.update(Msg::Fact(Fact::Event(Event::Assistant {
                    text: "Final answer".to_owned(),
                })));
            },
        ],
    );
    assert!(shown[0].contains("First word"), "{}", shown[0]);
    assert!(shown[1].contains("First word second word"), "{}", shown[1]);
    assert!(shown[2].contains("Final answer"), "{}", shown[2]);
    assert!(!shown[2].contains("First word"), "{}", shown[2]);
}

#[test]
fn a_long_step_run_mounts_only_the_visible_terminal_window() {
    sigil::reactive::Owner::scope("summonr-scale-steps", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 24);
        app.update(Msg::Intent(Intent::User("audit steps".to_owned())));
        for at in 0..60 {
            app.update(Msg::Fact(Fact::Event(Event::Reasoning {
                text: "next check".to_owned(),
            })));
            let id = format!("call_{at}");
            let args = format!(r#"{{"command":"step-cmd-{at:03}"}}"#);
            app.update(Msg::Fact(Fact::Event(Event::ToolCall {
                id: id.clone(),
                name: "run_command".to_owned(),
                arguments: args,
            })));
            app.update(Msg::Fact(Fact::Event(Event::ToolResult {
                id,
                success: true,
                mutated: false,
                output: "ok".to_owned(),
                millis: Some(1),
                patch: None,
            })));
        }
        app.refresh();
        let mut host = Host::new(HostMode::Cell);
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 24);
        for _ in 0..3 {
            screen.frame(&mut host, [], (100, 24)).expect("frame");
        }
        let grid = screen
            .frame(&mut host, [], (100, 24))
            .expect("frame")
            .text();
        let a11y = host.accessibility_text();
        assert!(grid.contains("step-cmd-059"), "{grid}");
        assert!(
            !a11y.contains("step-cmd-000"),
            "offscreen step rows stayed mounted:\n{a11y}"
        );
    });
}
