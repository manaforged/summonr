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

const REVIEW: &str = "Review PR #12";
const SURFACE: &str = r#"[{"version":"1.0","createSurface":{"surfaceId":"queue","catalogId":"https://a2ui.org/specification/v1_0/catalogs/basic/catalog.json"}},{"version":"1.0","updateComponents":{"surfaceId":"queue","components":[
  {"id":"root","component":"List","children":{"componentId":"pr","path":"/prs"}},
  {"id":"pr","component":"Row","children":["review","approve"]},
  {"id":"review","component":"Button","child":"review-label",
   "action":{"event":{"name":"start","context":{"prompt":{"@path":"prompt"},"pr":{"@path":"number"}}}}},
  {"id":"review-label","component":"Text","text":{"@path":"title"}},
  {"id":"approve","component":"Button","child":"approve-label",
   "action":{"event":{"name":"approve","context":{"pr":{"@path":"number"}}}}},
  {"id":"approve-label","component":"Text","text":"Approve"}]}},
 {"version":"1.0","updateDataModel":{"surfaceId":"queue","path":"/","value":
  {"prs":[{"title":"Fix cart rounding","number":12,"prompt":"Review PR #12"}]}}}]"#;

fn queue(app: &super::super::App) {
    settings(graded())(app);
    app.window.sidebar.set(true);
    app.update(Msg::Fact(Fact::Panels(vec![Panel {
        name: "queue".to_owned(),
        title: "Review queue".to_owned(),
        kind: Fill::Data,
        place: Place::Side,
        command: vec!["true".to_owned()],
        every_secs: None,
        root: None,
        problem: None,
    }])));
    app.update(Msg::Fact(Fact::Panel {
        name: "queue".to_owned(),
        result: Ok(Content::Surface(SURFACE.to_owned())),
    }));
}

#[test]
fn an_a2ui_button_in_a_side_panel_starts_an_agent_with_its_bound_prompt() {
    if !in_window("panels::an_a2ui_button_in_a_side_panel_starts_an_agent_with_its_bound_prompt") {
        return;
    }
    let seen = steps(
        queue,
        vec![
            Step::Click("Fix cart rounding"),
            Step::Wait(Box::new(|app| {
                app.update(Msg::Fact(Fact::Session("01a1000000000000000000000000ffff".to_owned())));
            })),
        ],
        |_| (),
    );
    assert!(seen.sent.iter().any(|cmd| matches!(cmd, Cmd::Open(_))), "a new agent opens");
    assert_eq!(prompts(&seen.sent), [REVIEW], "the bound prompt goes to the new agent");
}

#[test]
fn a_team_event_in_a_side_panel_goes_back_to_the_panel_with_its_context() {
    if !in_window("panels::a_team_event_in_a_side_panel_goes_back_to_the_panel_with_its_context") {
        return;
    }
    let seen = steps(queue, vec![Step::Click("Approve")], |_| ());
    let acted: Vec<_> = seen
        .sent
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::ActPanel { name, event, context } => Some((name.clone(), event.clone(), context.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        acted,
        [("queue".to_owned(), "approve".to_owned(), serde_json::json!({"pr": 12}))]
    );
    assert!(!seen.sent.iter().any(|cmd| matches!(cmd, Cmd::Open(_))), "no agent opens");
}

fn live_panel(place: Place) -> Panel {
    Panel {
        name: "live".to_owned(),
        title: "Live".to_owned(),
        kind: Fill::Stream,
        place,
        command: vec!["true".to_owned()],
        every_secs: None,
        root: None,
        problem: None,
    }
}

fn opened_and_closed(sent: &[Cmd]) -> (Vec<String>, Vec<String>, usize) {
    let mut opened = Vec::new();
    let mut closed = Vec::new();
    let mut reruns = 0;
    for cmd in sent {
        match cmd {
            Cmd::OpenPanel(name) => opened.push(name.clone()),
            Cmd::ClosePanel(name) => closed.push(name.clone()),
            Cmd::RunPanel(_) => reruns += 1,
            _ => {}
        }
    }
    (opened, closed, reruns)
}

#[test]
fn a_stream_side_panel_starts_while_it_shows_and_stops_when_it_goes() {
    if !in_window("panels::a_stream_side_panel_starts_while_it_shows_and_stops_when_it_goes") {
        return;
    }
    let seen = steps(
        |app| {
            settings(graded())(app);
            app.window.sidebar.set(true);
            app.update(Msg::Fact(Fact::Panels(vec![live_panel(Place::Side)])));
        },
        vec![Step::Wait(Box::new(|app| {
            app.update(Msg::Fact(Fact::Panels(Vec::new())));
        }))],
        |_| (),
    );
    assert_eq!(
        opened_and_closed(&seen.sent),
        (vec!["live".to_owned()], vec!["live".to_owned()], 0)
    );
}
