use super::{State, busy_info, frame_with};
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
    let shown = frame_with(busy_info(), 160, 60, State::Idle, |app| {
        app.replace("/usage".to_owned());
        app.submit();
        app.update(Msg::Usage(Ok(vec![
            spend("glm-5.3-flash", "/w/summonr", 2_000_000),
            spend("deepseek-v4-flash", "/w/website", 500_000),
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
        "website",
    ] {
        assert!(shown.contains(part), "missing {part:?}:\n{shown}");
    }
}
