use super::{State, busy_info, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

fn footer(lines: &[String]) -> String {
    lines
        .iter()
        .rfind(|line| line.contains("ctx"))
        .cloned()
        .unwrap_or_default()
}

#[test]
fn the_footer_shows_spend_beside_the_meters_and_no_rate() {
    let lines = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        app.update(Msg::Event(Event::Usage {
            prompt_tokens: 1200,
            completion_tokens: 300,
            reasoning_tokens: 0,
            cached_tokens: 800,
            millis: Some(2000),
            prefix_hash: String::new(),
            cost: Some(0.0412),
        }));
    });
    let line = footer(&lines);
    let spend = line.find("$0.04").unwrap_or_else(|| panic!("{line}"));
    let ctx = line.find("ctx").unwrap_or_default();
    assert!(spend < ctx, "{line}");
    assert!(!line.contains("tok/s"), "{line}");
}
