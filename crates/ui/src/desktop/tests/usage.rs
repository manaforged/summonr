use summonr_agent::hub::{Cmd, Fact, Msg};
use summonr_agent::session::{HOUR, Spend};

use super::super::settings::{Page, open};
use sigil::events::{Key, ModifiersState};

use super::harness::{Step, named, steps, typed};
use super::in_window;

fn spent(model: &str, workspace: &str, local: bool, cost: f64) -> Spend {
    Spend {
        model: model.to_owned(),
        hour: summonr_core::clock::now() / HOUR * HOUR,
        local,
        workspace: workspace.to_owned(),
        input: 40_000,
        cached: 30_000,
        output: 2_000,
        cost: Some(cost),
        turns: 3,
        millis: 30_000,
        sessions: 1,
    }
}

fn arrive(app: &super::super::App) {
    app.update(Msg::Fact(Fact::Usage(Ok(vec![
        spent("claude-opus-5-5", "/work/alpha", true, 1.25),
        spent("glm-5", "/work/beta", false, 0.25),
    ]))));
}

#[test]
fn the_usage_page_totals_spend_by_model_and_repository() {
    if !in_window("usage::the_usage_page_totals_spend_by_model_and_repository") {
        return;
    }
    let seen = steps(
        |app| open(app, Page::Usage),
        vec![Step::Wait(Box::new(arrive))],
        |_| (),
    );
    assert!(seen.sent.iter().any(|cmd| matches!(cmd, Cmd::ListUsage)));
    for shown in ["$1.50", "84.0k", "6", "75%", "claude-opus-5-5", "glm-5", "alpha", "beta"] {
        assert!(seen.shows(|text| text == shown), "{shown}: {}", seen.text());
    }
    let here = steps(
        |app| open(app, Page::Usage),
        vec![Step::Wait(Box::new(arrive)), Step::Click("This repository")],
        |_| (),
    );
    assert!(here.shows(|text| text == "$1.25"), "{}", here.text());
    assert!(!here.shows(|text| text == "glm-5"), "{}", here.text());
}

#[test]
fn the_usage_command_opens_the_usage_page_in_the_window() {
    if !in_window("usage::the_usage_command_opens_the_usage_page_in_the_window") {
        return;
    }
    let mut command: Vec<Step> = typed("/usage").into_iter().map(Step::Key).collect();
    command.push(Step::Key(named(Key::Enter, ModifiersState::empty())));
    let seen = steps(|_| {}, command, |app| app.settings_open.peek());
    assert!(seen.read, "{}", seen.text());
    assert!(seen.shows(|text| text == "Reading sessions…"), "{}", seen.text());
    assert!(seen.sent.iter().any(|cmd| matches!(cmd, Cmd::ListUsage)));
}
