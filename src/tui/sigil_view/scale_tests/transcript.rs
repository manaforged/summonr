use super::super::app::App;
use super::screens;
use crate::tools::Event;
use crate::tui::Msg;

#[test]
fn mounted_terminal_prose_streams_and_replaces_the_final_answer() {
    let shown = screens(
        100,
        30,
        |app| {
            app.update(Msg::Event(Event::Delta {
                text: "First word".to_owned(),
            }));
        },
        &[
            &|app: &App| {
                app.update(Msg::Event(Event::Delta {
                    text: " second word".to_owned(),
                }));
            },
            &|app: &App| {
                app.update(Msg::Event(Event::Assistant {
                    text: "Final answer".to_owned(),
                }));
            },
        ],
    );
    assert!(shown[0].contains("First word"), "{}", shown[0]);
    assert!(shown[1].contains("First word second word"), "{}", shown[1]);
    assert!(shown[2].contains("Final answer"), "{}", shown[2]);
    assert!(!shown[2].contains("First word"), "{}", shown[2]);
}
