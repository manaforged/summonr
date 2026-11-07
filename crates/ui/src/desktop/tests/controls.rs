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
fn the_strip_counts_tokens_and_the_busy_row_names_the_work() {
    if !in_window("controls::the_strip_counts_tokens_and_the_busy_row_names_the_work") {
        return;
    }
    let seen = drive(
        |app| {
            used(app);
            app.update(Msg::Fact(Fact::Busy(Some(
                summonr_agent::hub::Activity::Turn,
            ))));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.shows(|text| text == "1.5k tokens"), "{}", seen.text());
    assert!(
        seen.shows(|text| text.contains("Thinking…")),
        "{}",
        seen.text()
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
