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
    let fresh = drive(|_| (), Vec::new(), |_| ());
    assert!(
        !fresh
            .controls
            .iter()
            .any(|(_, label, _)| label.starts_with("Context used"))
    );
}

#[test]
fn the_approval_chip_names_the_mode_and_toggles_it() {
    if !in_window("controls::the_approval_chip_names_the_mode_and_toggles_it") {
        return;
    }
    let auto = steps(
        settings(graded()),
        vec![Step::Click("Auto-approve")],
        |_| (),
    );
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
