use sigil::terminal::Ink;

use super::super::canvas::palette;
use super::{State, busy_info, frame_with, paint};
use crate::session::Spend;
use crate::tui::Msg;

fn spend(model: &str, workspace: &str, input: u64) -> Spend {
    let hour = crate::mail::now() / 3600 * 3600;
    Spend {
        model: model.to_owned(),
        hour,
        workspace: workspace.to_owned(),
        input,
        cached: input * 9 / 10,
        output: 1000,
        turns: 4,
        millis: 8000,
        sessions: 1,
        ..Spend::default()
    }
}

#[test]
fn the_usage_page_shows_totals_trend_models_and_workspaces() {
    let shown = frame_with(busy_info(), 160, 90, State::Idle, |app| {
        app.replace("/usage".to_owned());
        app.submit();
        app.update(Msg::Usage(Ok(vec![
            spend("glm-5.3-flash", "/w/summonr", 2_000_000),
            spend("deepseek-v4-flash", "/w/webshop", 500_000),
        ])));
    })
    .join("\n");
    for part in [
        "cache hit",
        "90.0%",
        "8 turns",
        "Tokens per",
        "Models",
        "glm-5.3-flash",
        "Workspaces",
        "webshop",
    ] {
        assert!(shown.contains(part), "missing {part:?}:\n{shown}");
    }
}

#[test]
fn a_shared_day_paints_each_models_color() {
    let grid = paint(busy_info(), 140, 60, State::Idle, |app| {
        app.replace("/usage".to_owned());
        app.submit();
        app.update(Msg::Usage(Ok(vec![
            spend("glm-5.3-flash", "/w/summonr", 2_000_000),
            spend("deepseek-v4-flash", "/w/webshop", 1_000_000),
        ])));
    });
    let accent = Ink::Exact(palette().accent);
    let yellow = Ink::Exact(palette().yellow);
    let stacked = (0..grid.cols()).any(|col| {
        let mut saw_accent = false;
        let mut saw_yellow = false;
        for row in 0..grid.rows() {
            let Some(cell) = grid.get(col, row) else {
                continue;
            };
            saw_accent |= cell.bg == accent;
            saw_yellow |= cell.bg == yellow;
        }
        saw_accent && saw_yellow
    });
    assert!(
        stacked,
        "a day that both models ran is still one color:\n{}",
        grid.text()
    );
}

#[test]
fn a_narrow_usage_page_keeps_a_model_off_the_workspace_row() {
    let lines = frame_with(busy_info(), 84, 70, State::Idle, |app| {
        app.replace("/usage".to_owned());
        app.submit();
        app.update(Msg::Usage(Ok(vec![
            spend("glm-5.3-flash", "/w/summonr", 2_000_000),
            spend("deepseek-v4-flash", "/w/webshop", 1_000_000),
        ])));
    });
    let Some(row) = lines.iter().find(|line| line.contains("glm-5.3-flash")) else {
        panic!("{}", lines.join("\n"));
    };
    assert!(
        !row.contains("webshop"),
        "model row ran into the workspace:\n{row}"
    );
}
