use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::catalog::Model;
use crate::recent::Seat;
use crate::tui::Msg;

fn session(id: &str, parent: Option<&str>) -> crate::session::SessionInfo {
    crate::session::SessionInfo {
        id: id.to_string(),
        path: std::path::PathBuf::new(),
        cwd: Some("/work".to_string()),
        model: Some("m".to_string()),
        preview: format!("Session {id}"),
        name: None,
        parent: parent.map(str::to_string),
        depth: 0,
    }
}

#[test]
fn a_session_delete_confirmation_is_visible_with_existing_rows() {
    let screen = picker(80, |app| {
        app.open_sessions(false);
        app.update(Msg::Sessions(Ok(vec![
            (session("first", None), true),
            (session("second", None), true),
        ])));
        app.delete_session();
    });
    assert!(screen.contains("alt+d again deletes first"), "{screen}");
}

#[test]
fn a_changed_target_and_a_reopened_picker_need_a_new_delete_confirmation() {
    use crate::tui::Cmd;
    use sigil::reactive::Runtime;
    use tokio::sync::mpsc::unbounded_channel;
    Runtime::scope("session-delete", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        let rows = vec![
            (session("first", None), true),
            (session("second", None), true),
        ];
        app.open_sessions(false);
        assert!(matches!(sent.try_recv(), Ok(Cmd::ListSessions)));
        app.update(Msg::Sessions(Ok(rows.clone())));
        app.delete_session();
        app.overlay_move(1);
        app.delete_session();
        assert!(
            sent.try_recv().is_err(),
            "changing the target deleted without confirmation"
        );
        app.overlay_close();
        app.open_sessions(false);
        assert!(matches!(sent.try_recv(), Ok(Cmd::ListSessions)));
        app.update(Msg::Sessions(Ok(rows)));
        app.overlay_move(1);
        app.delete_session();
        assert!(
            sent.try_recv().is_err(),
            "reopening reused the old confirmation"
        );
        app.delete_session();
        assert!(matches!(sent.try_recv(), Ok(Cmd::DeleteSession(id)) if id == "second"));
    });
}

#[test]
fn cycling_to_related_sessions_selects_the_current_branch() {
    let _screen = picker(100, |app| {
        app.open_sessions(false);
        let current = app.model.peek().session.clone();
        app.update(Msg::Sessions(Ok(vec![
            (session("parent", None), true),
            (session(&current, Some("parent")), true),
        ])));
        app.sessions.scope();
        app.sessions.scope();
        assert_eq!(app.sessions.selected.peek(), 1);
        assert_eq!(app.sessions.rows()[1].id, current);
    });
}

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

#[test]
fn the_model_picker_fills_a_tall_terminal() {
    let models: Vec<(String, Option<u64>)> = (0..90)
        .map(|index| (format!("model-{index:02}"), days_ago(20)))
        .collect();
    let rows: Vec<(&str, Option<u64>)> = models
        .iter()
        .map(|(id, release)| (id.as_str(), *release))
        .collect();
    let screen = frame_with(busy_info(), 165, 85, State::Idle, |app| open(app, &rows, 0));
    let top = screen.iter().position(|line| line.contains('┏'));
    let bottom = screen.iter().rposition(|line| line.contains('┗'));
    let tall = top.zip(bottom).map(|(top, bottom)| bottom - top + 1);
    assert_eq!(tall, Some(83), "{}", screen.join("\n"));
    assert!(
        screen.iter().any(|line| line.contains("model-60")),
        "{}",
        screen.join("\n")
    );
}
