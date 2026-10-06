use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::catalog::Model;
use crate::recent::Seat;
use crate::tui::Msg;

fn days_ago(days: u64) -> Option<u64> {
    let today = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        / 86_400;
    Some(today - days)
}

fn open(app: &App, rows: &[(&str, Option<u64>)], pinned: usize) {
    app.replace("/model".to_owned());
    app.submit();
    let seat = |id: &str| Seat::new("openrouter", id);
    app.update(Msg::Models {
        request: 1,
        rows: rows.iter().map(|(id, _)| seat(id)).collect(),
        facts: rows
            .iter()
            .map(|(id, release)| {
                let mut model = Model::named(*id);
                model.release = *release;
                (seat(id), model)
            })
            .collect(),
        groups: BTreeMap::new(),
        causes: BTreeMap::new(),
        pinned,
        recent: 0,
        loading: false,
        error: String::new(),
    });
}

fn picker(cols: u16, setup: impl FnOnce(&App)) -> String {
    frame_with(busy_info(), cols, 30, State::Idle, setup).join("\n")
}

#[test]
fn the_model_picker_hides_models_released_over_six_months_ago() {
    let screen = picker(140, |app| {
        open(
            app,
            &[
                ("fresh-model", days_ago(20)),
                ("stale-model", days_ago(400)),
            ],
            0,
        );
    });
    assert!(screen.contains("fresh-model"), "{screen}");
    assert!(!screen.contains("stale-model"), "{screen}");
    assert!(screen.contains("show 1 older model ·"), "{screen}");
}

#[test]
fn showing_older_models_lists_them_in_the_picker() {
    let screen = picker(140, |app| {
        open(
            app,
            &[
                ("fresh-model", days_ago(20)),
                ("stale-model", days_ago(400)),
            ],
            0,
        );
        app.toggle_older_models();
    });
    assert!(screen.contains("stale-model"), "{screen}");
    assert!(screen.contains("hide 1 older model ·"), "{screen}");
}

#[test]
fn a_favorite_older_model_stays_in_the_picker() {
    let screen = picker(140, |app| {
        open(
            app,
            &[
                ("stale-favorite", days_ago(400)),
                ("fresh-model", days_ago(20)),
            ],
            1,
        );
    });
    assert!(screen.contains("favorites"), "{screen}");
    assert!(screen.contains("stale-favorite"), "{screen}");
}

#[test]
fn searching_the_picker_finds_older_models() {
    let screen = picker(140, |app| {
        open(
            app,
            &[
                ("fresh-model", days_ago(20)),
                ("stale-model", days_ago(400)),
            ],
            0,
        );
        app.replace_model_draft("stale".to_owned());
    });
    assert!(screen.contains("stale-model"), "{screen}");
}

#[test]
fn a_model_without_a_release_date_stays_in_the_picker() {
    let screen = picker(140, |app| {
        open(app, &[("undated-model", None)], 0);
    });
    assert!(screen.contains("undated-model"), "{screen}");
    assert!(!screen.contains("older model"), "{screen}");
}

#[test]
fn the_wide_picker_shows_the_release_day_of_the_selected_model() {
    let screen = picker(140, |app| {
        open(app, &[("leap-model", Some(19_782))], 1);
        app.overlay_move(1);
    });
    let row = screen
        .lines()
        .find(|line| line.contains("released"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(row.contains("2024-02-29"), "{screen}");
}

#[test]
fn the_model_picker_uses_the_width_of_a_wide_terminal() {
    let screen = picker(140, |app| open(app, &[("fresh-model", days_ago(20))], 0));
    let top = screen
        .lines()
        .find(|line| line.contains('┏'))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(top.matches('━').count() >= 110, "{screen}");
}

#[test]
fn alt_p_adds_the_selected_model_to_favorites() {
    let screen = picker(140, |app| {
        open(app, &[("fresh-model", days_ago(20))], 0);
        app.overlay_move(1);
        app.pin_model();
    });
    assert!(screen.contains("★ favorite fresh-model"), "{screen}");
}
