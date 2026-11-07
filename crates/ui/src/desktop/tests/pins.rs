use super::harness::{Step, drive, steps};
use super::in_window;
use super::sidebar::two_chats;
use summonr_agent::prefs::Action;

fn pin_from_the_palette(label: &'static str) -> Vec<Step> {
    vec![
        Step::Click("Search"),
        Step::Click("Pin to sidebar…"),
        Step::Click(label),
    ]
}

#[test]
fn an_action_pinned_from_the_palette_stays_in_the_sidebar_after_a_reload() {
    if !in_window("pins::an_action_pinned_from_the_palette_stays_in_the_sidebar_after_a_reload")
    {
        return;
    }
    let pinned = steps(two_chats, pin_from_the_palette("Keyboard shortcuts"), |app| {
        app.model.peek().prefs.clone()
    });
    assert!(
        pinned.shows(|text| text == "Keyboard shortcuts"),
        "{}",
        pinned.text()
    );
    let dir = tempfile::tempdir().expect("config folder");
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "").expect("config file");
    summonr_agent::prefs::save_in(&path, &pinned.read).expect("save");
    let loaded = summonr_agent::prefs::load_from(&path).expect("load");
    assert_eq!(loaded.sidebar, pinned.read.sidebar);
    let seen = drive(
        move |app| {
            two_chats(app);
            app.model_mut(|model| model.prefs = loaded);
        },
        Vec::new(),
        |_| (),
    );
    for row in ["Keyboard shortcuts", "New terminal", "Search"] {
        assert!(seen.shows(|text| text == row), "{row}: {}", seen.text());
    }
}

#[test]
fn unpinning_a_default_row_takes_it_out_of_the_sidebar() {
    if !in_window("pins::unpinning_a_default_row_takes_it_out_of_the_sidebar") {
        return;
    }
    let seen = steps(
        two_chats,
        vec![Step::RightClick("New terminal"), Step::Click("Unpin")],
        |app| app.model.peek().prefs.sidebar.clone(),
    );
    assert!(!seen.shows(|text| text == "New terminal"), "{}", seen.text());
    assert!(seen.shows(|text| text == "Search"), "{}", seen.text());
    assert_eq!(seen.read, vec![Action::NewAgent, Action::Search]);
}

#[test]
fn a_pinned_row_opens_its_pane() {
    if !in_window("pins::a_pinned_row_opens_its_pane") {
        return;
    }
    let pinned = steps(two_chats, pin_from_the_palette("Jobs"), |app| {
        app.target().layout.peek().secondary
    });
    assert_eq!(pinned.read, None, "{}", pinned.text());
    assert!(!pinned.shows(|text| text == "Peers"), "{}", pinned.text());
    let mut actions = pin_from_the_palette("Jobs");
    actions.push(Step::Click("Jobs"));
    let seen = steps(two_chats, actions, |app| app.target().layout.peek().secondary);
    assert_eq!(seen.read, Some(super::super::Pane::Jobs), "{}", seen.text());
}
