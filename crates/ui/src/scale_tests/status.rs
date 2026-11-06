use super::screens;
use summonr_agent::hub::{Activity, Fact, Msg};
use summonr_agent::tools::Event;

fn status(screen: &str) -> String {
    screen
        .lines()
        .find(|line| line.contains("esc to interrupt"))
        .map(str::trim)
        .unwrap_or_default()
        .to_owned()
}

fn event(app: &super::super::app::App, event: Event) {
    app.update(Msg::Fact(Fact::Event(event)));
}

#[test]
fn the_status_line_names_each_phase_of_a_turn() {
    let shown = screens(
        110,
        30,
        |app| app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn)))),
        &[
            &|app| {
                event(
                    app,
                    Event::Reasoning {
                        text: "weigh it".to_owned(),
                    },
                );
            },
            &|app| {
                event(
                    app,
                    Event::Delta {
                        text: "Half an".to_owned(),
                    },
                );
            },
            &|app| {
                event(
                    app,
                    Event::Usage {
                        prompt_tokens: 900,
                        completion_tokens: 40,
                        reasoning_tokens: 0,
                        cached_tokens: 0,
                        cache_write_tokens: 0,
                        millis: Some(800),
                        prefix_hash: String::new(),
                        cost: None,
                        side: false,
                    },
                );
            },
            &|app| {
                event(
                    app,
                    Event::Assistant {
                        text: "Half an answer.".to_owned(),
                    },
                );
            },
            &|app| app.update(Msg::Fact(Fact::Busy(None))),
        ],
    );
    let lines: Vec<String> = shown.iter().map(|screen| status(screen)).collect();
    let verbs: Vec<&str> = lines
        .iter()
        .map(|line| line.split_whitespace().nth(1).unwrap_or_default())
        .collect();
    assert_eq!(
        verbs,
        [
            "Thinking…",
            "Thinking…",
            "Writing…",
            "Finishing…",
            "Finishing…",
            ""
        ],
        "{lines:#?}"
    );
    assert!(
        lines[..5]
            .iter()
            .all(|line| line.starts_with(|ch: char| ('⠁'..='⣿').contains(&ch))),
        "{lines:#?}"
    );
}
