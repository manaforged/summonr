use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::provider::Effort;
use crate::tui::Cmd;

#[test]
fn shift_tab_cycles_reasoning_effort() {
    Runtime::scope("shift-tab-effort", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        let mut snapshot = app.model.peek().snapshot.clone();
        snapshot.levels = vec![
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::XHigh,
            Effort::Max,
        ];
        app.update(crate::tui::Msg::Settings(snapshot));
        let mut stops = Vec::new();
        for _ in 0..4 {
            app.cycle_effort(1);
            stops.push(app.model.get().snapshot.effort);
        }
        let mut last = None;
        while let Ok(command) = sent.try_recv() {
            if let Cmd::SetEffort(effort) = command {
                last = effort;
            }
        }
        let expected = [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh];
        assert_eq!(stops, expected.map(Some));
        assert_eq!(last, Some(Effort::XHigh));
    });
}

#[test]
fn tab_cycles_back_to_the_lowest_supported_level() {
    Runtime::scope("tab-effort-wrap", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        let mut snapshot = app.model.peek().snapshot.clone();
        snapshot.levels = vec![Effort::Low, Effort::Medium, Effort::High];
        app.update(crate::tui::Msg::Settings(snapshot));
        for _ in 0..3 {
            app.cycle_effort(1);
        }
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::High));
        app.cycle_effort(1);
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::Low));
        app.cycle_effort(-1);
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::High));
    });
}

#[test]
fn slash_plan_is_refused_while_plan_mode_is_off() {
    let lines = frame_with(busy_info(), 100, 24, State::Idle, |app| {
        app.replace("/plan".to_owned());
        app.submit();
    });
    assert!(
        lines.iter().any(|line| line.contains("plan mode is off")),
        "{}",
        lines.join("\n")
    );
}
