use sigil::prelude::Role;

use super::harness::{Step, drive, graded, settings, steps};
use super::in_window;
use summonr_agent::hub::{Cmd, Fact, Msg, Snapshot};

fn used(app: &super::super::App) {
    app.update(Msg::Fact(Fact::Settings(Snapshot {
        context_window: Some(200_000),
        ..graded()
    })));
    app.update(Msg::Fact(Fact::Event(summonr_agent::tools::Event::Usage {
        prompt_tokens: 1500,
        completion_tokens: 20,
        reasoning_tokens: 0,
        cached_tokens: 0,
        cache_write_tokens: 0,
        millis: None,
        prefix_hash: String::new(),
        cost: None,
        side: false,
    })));
}

#[test]
fn the_composer_shows_how_full_the_context_is() {
    if !in_window("controls::the_composer_shows_how_full_the_context_is") {
        return;
    }
    let seen = drive(used, Vec::new(), |_| ());
    assert!(
        seen.controls
            .iter()
            .any(|(_, label, _)| label == "Context used 1%"),
        "{:?}",
        seen.controls
    );
    assert!(
        seen.controls
            .iter()
            .any(|(_, label, _)| label == "Cache hit 0%"),
        "{:?}",
        seen.controls
    );
    let fresh = drive(|_| (), Vec::new(), |_| ());
    assert!(
        !fresh.controls.iter().any(
            |(_, label, _)| label.starts_with("Context used") || label.starts_with("Cache hit")
        )
    );
}

#[test]
fn a_running_chat_shows_its_work_time_and_tokens_above_the_composer() {
    if !in_window("controls::a_running_chat_shows_its_work_time_and_tokens_above_the_composer") {
        return;
    }
    let seen = drive(
        |app| {
            used(app);
            app.update(Msg::Fact(Fact::Busy(Some(
                summonr_agent::hub::Activity::Turn,
            ))));
            app.update(Msg::Fact(Fact::Event(summonr_agent::Event::Reasoning {
                text: "a".repeat(4000),
            })));
        },
        Vec::new(),
        |_| (),
    );
    let at = |wanted: &dyn Fn(&str) -> bool| {
        seen.texts
            .iter()
            .position(|text| wanted(text))
            .unwrap_or_else(|| panic!("missing from {}", seen.text()))
    };
    let tokens = at(&|text| text == "2.5k tokens");
    let work = at(&|text| text.contains("Thinking…"));
    let clock = at(&|text| {
        text.strip_suffix('s')
            .is_some_and(|secs| !secs.is_empty() && secs.chars().all(|ch| ch.is_ascii_digit()))
    });
    let composer = at(&|text| text == "What should this agent do?");
    assert!(
        work < tokens && tokens < clock && clock < composer,
        "{}",
        seen.text()
    );
    assert_eq!(
        seen.texts
            .iter()
            .filter(|text| text.ends_with(" tokens"))
            .count(),
        1,
        "{}",
        seen.text()
    );
}

#[test]
fn an_idle_chat_keeps_its_tokens_under_the_composer() {
    if !in_window("controls::an_idle_chat_keeps_its_tokens_under_the_composer") {
        return;
    }
    let seen = drive(used, Vec::new(), |_| ());
    assert!(seen.shows(|text| text == "1.5k tokens"), "{}", seen.text());
    assert!(
        !seen.shows(|text| text.contains("Thinking…")),
        "{}",
        seen.text()
    );
}

#[test]
fn the_model_chip_shows_effort_and_its_menu_changes_it() {
    if !in_window("controls::the_model_chip_shows_effort_and_its_menu_changes_it") {
        return;
    }
    let seen = steps(
        settings(graded()),
        vec![Step::Click("Model"), Step::Click("High")],
        |app| app.model.peek().snapshot.effort,
    );
    assert_eq!(seen.read, Some(summonr_agent::provider::Effort::High));
    let none = drive(
        settings(Snapshot {
            levels: Vec::new(),
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert!(
        !none.shows(|text| text.contains(" · Low")),
        "{}",
        none.text()
    );
    let shown = drive(settings(graded()), Vec::new(), |_| ());
    assert!(
        shown.shows(|text| text.ends_with(" · Low")),
        "{}",
        shown.text()
    );
}

#[test]
fn a_chat_held_elsewhere_offers_take_over_until_the_view_changes() {
    if !in_window("controls::a_chat_held_elsewhere_offers_take_over_until_the_view_changes") {
        return;
    }
    let held = |app: &super::super::App| {
        app.update(Msg::Fact(Fact::Held("abc".to_owned())));
    };
    let taken = steps(held, vec![Step::Click("Take over")], |_| ());
    assert!(
        taken
            .sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::TakeOver(id) if id == "abc"))
    );
    let dismissed = steps(held, vec![Step::Click("Not now")], |app| app.held.peek());
    assert_eq!(dismissed.read, None);
    let moved = drive(
        |app| {
            held(app);
            app.update(Msg::Intent(summonr_agent::hub::Intent::Reset));
        },
        Vec::new(),
        |_| (),
    );
    assert!(moved.named(Role::Button, "Take over").is_empty());
}

#[test]
fn a_taken_chat_says_another_window_took_it() {
    if !in_window("controls::a_taken_chat_says_another_window_took_it") {
        return;
    }
    let seen = drive(
        |app| app.update(Msg::Fact(Fact::TakenOver)),
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "Another window took over this chat"),
        "{}",
        seen.text()
    );
}

#[test]
fn the_approval_chip_names_the_mode_and_toggles_it() {
    if !in_window("controls::the_approval_chip_names_the_mode_and_toggles_it") {
        return;
    }
    let auto = steps(settings(graded()), vec![Step::Click("Allow all")], |_| ());
    assert!(
        auto.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::ToggleApproval))
    );
    let asking = drive(
        settings(Snapshot {
            approval: true,
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert_eq!(asking.named(Role::Button, "Ask first"), vec![false]);
    let strict = drive(
        settings(Snapshot {
            strict: true,
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert_eq!(strict.named(Role::Button, "Strict"), vec![true]);
}
