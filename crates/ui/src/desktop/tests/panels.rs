use summonr_agent::hub::{Cmd, Fact, Msg};
use summonr_agent::panel::{Action, Content, Fill, Item, Mood, Panel, Place};

use super::harness::{Step, graded, settings, steps};
use super::in_window;

const ISSUE: &str = "Checkout total off by a cent";
const PROMPT: &str = "Fix issue #212: Checkout total off by a cent";

fn team(place: Place) -> impl FnOnce(&super::super::App) {
    move |app| {
        settings(graded())(app);
        app.window.sidebar.set(true);
        app.update(Msg::Fact(Fact::Panels(vec![Panel {
            name: "team".to_owned(),
            title: "Team".to_owned(),
            kind: Fill::Data,
            place,
            command: vec!["true".to_owned()],
            every_secs: None,
            root: None,
            problem: None,
        }])));
        app.update(Msg::Fact(Fact::Panel {
            name: "team".to_owned(),
            result: Ok(Content::List {
                rows: vec![Item {
                    title: ISSUE.to_owned(),
                    meta: Some("#212 · assigned to you".to_owned()),
                    tone: Mood::Warning,
                    action: Some(Action::Start { prompt: PROMPT.to_owned() }),
                }],
            }),
        }));
    }
}

fn prompts(sent: &[Cmd]) -> Vec<String> {
    sent.iter()
        .filter_map(|cmd| match cmd {
            Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input) => Some(input.text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_side_panel_row_starts_an_agent_with_its_prompt() {
    if !in_window("panels::a_side_panel_row_starts_an_agent_with_its_prompt") {
        return;
    }
    let seen = steps(
        team(Place::Side),
        vec![
            Step::Click(ISSUE),
            Step::Wait(Box::new(|app| {
                app.update(Msg::Fact(Fact::Session("01a1000000000000000000000000eeee".to_owned())));
            })),
        ],
        |_| (),
    );
    assert!(seen.shows(|text| text == "Team"), "the panel's title shows: {}", seen.text());
    assert!(
        seen.sent.iter().any(|cmd| matches!(cmd, Cmd::Open(_))),
        "a new agent opens"
    );
    assert_eq!(prompts(&seen.sent), [PROMPT], "the row's prompt goes to the new agent");
}

#[test]
fn a_right_panel_stays_out_of_the_sidebar() {
    if !in_window("panels::a_right_panel_stays_out_of_the_sidebar") {
        return;
    }
    let seen = steps(team(Place::Right), Vec::new(), |_| ());
    assert!(!seen.shows(|text| text == ISSUE), "{}", seen.text());
}
