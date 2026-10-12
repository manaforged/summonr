use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::busy_info;
use crate::provider::Effort;
use crate::tui::Cmd;

#[test]
fn cycling_effort_uses_supported_levels() {
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
fn tab_changes_effort_and_shift_tab_requests_approval_with_the_composer_focused() {
    use super::super::{app::Facet, view::root};
    use crate::tui::Msg;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sigil::{Host, events::EventRouter, terminal::Screen};
    use tokio::sync::mpsc::error::TryRecvError;

    Runtime::scope("mounted-tab-controls", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        app.resize(100, 30);
        let mut snapshot = app.model.peek().snapshot.clone();
        snapshot.levels = vec![Effort::Low, Effort::Medium, Effort::High];
        app.update(Msg::Settings(snapshot));
        app.replace("unchanged draft".to_owned());
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (100, 30))
                .expect("focus composer");
        }
        let shown = screen
            .frame(&mut host, &mut router, [], (100, 30))
            .expect("auto mode")
            .text();
        assert!(shown.contains("● auto"), "{shown}");
        assert!(!shown.contains("auto approval"), "{shown}");
        let tab = sigil::terminal::map_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .expect("Tab");
        screen
            .frame(&mut host, &mut router, [tab], (100, 30))
            .expect("press Tab");
        assert!(matches!(
            sent.try_recv(),
            Ok(Cmd::SetEffort(Some(Effort::Low)))
        ));
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::Low));
        let shifted =
            sigil::terminal::map_key(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
                .expect("Shift Tab");
        screen
            .frame(&mut host, &mut router, [shifted], (100, 30))
            .expect("press Shift Tab");
        assert!(matches!(sent.try_recv(), Ok(Cmd::ToggleApproval)));
        assert!(matches!(sent.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::Low));
        assert!(!app.model.get().snapshot.approval);
        assert_eq!(app.draft.peek(), "unchanged draft");
        let mut snapshot = app.model.peek().snapshot.clone();
        snapshot.approval = true;
        app.update(Msg::Settings(snapshot));
        let shown = screen
            .frame(&mut host, &mut router, [], (100, 30))
            .expect("approval mode")
            .text();
        assert!(shown.contains("● approval"), "{shown}");
        assert!(!shown.contains("auto · approval"), "{shown}");
        assert!(!shown.contains("auto approval"), "{shown}");
        app.replace("/sett".to_owned());
        let tab = sigil::terminal::map_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .expect("completion Tab");
        screen
            .frame(&mut host, &mut router, [tab], (100, 30))
            .expect("complete command");
        assert_eq!(app.draft.peek(), "/settings");
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::Low));
        assert!(matches!(sent.try_recv(), Err(TryRecvError::Empty)));
        app.begin_model_edit();
        assert!(matches!(sent.try_recv(), Ok(Cmd::ListModels { .. })));
        screen
            .frame(&mut host, &mut router, [], (100, 30))
            .expect("model overlay");
        let shifted =
            sigil::terminal::map_key(&KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
                .expect("overlay Shift Tab");
        screen
            .frame(&mut host, &mut router, [shifted], (100, 30))
            .expect("previous model filter");
        assert_eq!(app.picker.facet.peek(), Facet::Long);
        let tab = sigil::terminal::map_key(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .expect("overlay Tab");
        screen
            .frame(&mut host, &mut router, [tab], (100, 30))
            .expect("next model filter");
        assert_eq!(app.picker.facet.peek(), Facet::All);
        assert!(matches!(sent.try_recv(), Err(TryRecvError::Empty)));
        assert_eq!(app.model.get().snapshot.effort, Some(Effort::Low));
        assert!(app.model.get().snapshot.approval);
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
