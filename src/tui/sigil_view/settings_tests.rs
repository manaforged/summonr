use super::*;

#[test]
fn autocomplete_and_settings_keep_the_overlays() {
    sigil::reactive::Runtime::scope("mage-overlay-parity-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.replace("/".to_string());
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 32);
        let popup = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(
            popup.text().contains("› /help"),
            "popup missing:\n{}",
            popup.text()
        );
        assert!(popup.text().contains('╭') && popup.text().contains('╯'));

        app.settings_open.set(true);
        app.settings_sel.set(0);
        let settings = screen
            .frame(&mut host, &mut router, [], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(settings.text().contains("⚙ settings"));
        assert!(settings.text().contains("› mode"));
        assert!(settings.text().contains("interface"));
        assert!(
            settings
                .text()
                .contains("tab/↑↓ select · ←→ change · enter open · esc close")
        );
        assert!(has_color(
            &settings,
            Rgba8 {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            }
        ));

        router.focus_next(host.cx());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowDown)],
                (100, 32),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.settings_sel.peek(), 1);
        app.settings_sel.set(2);
        let increase = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('+'),
            KeyModifiers::NONE,
        ))
        .unwrap_or_else(|| panic!("+ did not map"));
        screen
            .frame(&mut host, &mut router, [increase], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetRounds(72))));
        let typed = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ))
        .unwrap_or_else(|| panic!("x did not map"));
        screen
            .frame(&mut host, &mut router, [typed], (100, 32))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "/", "settings changed the hidden draft");
    });
}

#[test]
fn settings_model_row_opens_the_provider_picker() {
    sigil::reactive::Runtime::scope("mage-settings-model-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.settings_open.set(true);
        app.settings_sel.set(4);
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(
            frame.contains("provider"),
            "provider group missing:\n{frame}"
        );
        assert!(frame.contains("endpoint"), "endpoint row missing:\n{frame}");
        assert!(frame.contains("glm-5.3"), "model row missing:\n{frame}");

        app.begin_model_edit();
        assert!(app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.update(Msg::Models(vec![
            "glm-5.3".to_string(),
            "glm-5.4".to_string(),
            "glm-5.4-air".to_string(),
        ]));
        let tree = app.clone();
        let editing = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("model picker 100x32:\n{editing}");
        }
        assert!(
            editing.contains("models"),
            "model picker is not visible:\n{editing}"
        );
        assert!(editing.contains("glm-5.4-air"));
        assert!(editing.contains("type to filter · enter use · esc close"));
        app.move_model(1);
        app.commit_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetModel(model)) if model == "glm-5.4"));
        assert!(!app.editing_model.peek());
    });
}

#[test]
fn model_picker_switches_endpoint() {
    sigil::reactive::Runtime::scope("mage-model-switch-endpoint-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.begin_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.update(Msg::Models(vec![
            "glm\tglm-5.3".to_string(),
            "qwen\tqwen3.8-27b-abliterated".to_string(),
        ]));
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(
            frame.contains("qwen") && frame.contains("glm"),
            "picker stayed on one endpoint:\n{frame}"
        );
        app.move_model(1);
        app.commit_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetEndpoint(name)) if name == "qwen"));
    });
}

#[test]
fn model_picker_shows_discovery_failures() {
    sigil::reactive::Runtime::scope("mage-model-error-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.begin_model_edit();
        app.update(Msg::ModelsFailed("connection refused".to_string()));
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(
            frame.contains("couldn't load the host list"),
            "missing error state:\n{frame}"
        );
        assert!(
            frame.contains("connection refused"),
            "missing error detail:\n{frame}"
        );
        assert!(
            frame.contains("glm-5.3"),
            "current model missing after a failed list:\n{frame}"
        );
    });
}

#[test]
fn settings_use_group_spacing_in_a_normal_terminal() {
    sigil::reactive::Runtime::scope("mage-settings-spacing-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(100, 32);
        app.refresh();
        app.settings_open.set(true);
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("settings 100x32:\n{frame}");
        }
        let rows: Vec<&str> = frame.lines().collect();
        let session = rows
            .iter()
            .position(|row| row.contains("session"))
            .unwrap_or_else(|| panic!("session group missing:\n{frame}"));
        let provider = rows
            .iter()
            .position(|row| row.contains("provider"))
            .unwrap_or_else(|| panic!("provider group missing:\n{frame}"));
        let interface = rows
            .iter()
            .position(|row| row.contains("interface"))
            .unwrap_or_else(|| panic!("interface group missing:\n{frame}"));

        assert!(
            provider >= session + 5,
            "session group is cramped:\n{frame}"
        );
        assert!(
            interface >= provider + 4,
            "provider group is cramped:\n{frame}"
        );
    });
}

#[test]
fn settings_model_edit_uses_enter_and_escape() {
    sigil::reactive::Runtime::scope("mage-settings-model-keys-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.settings_open.set(true);
        app.settings_sel.set(4);
        app.resize(80, 24);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.editing_model.peek());
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        let typed = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        ));
        screen
            .frame(&mut host, &mut router, typed, (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.model_draft.peek(), "x");

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Escape)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.editing_model.peek());
        assert!(rx.try_recv().is_err(), "cancel switched the model");

        app.begin_model_edit();
        assert!(matches!(rx.try_recv(), Ok(Cmd::ListModels)));
        app.update(Msg::Models(vec![
            "glm-5.3".to_string(),
            "glm-5.4".to_string(),
        ]));
        app.move_model(1);
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetModel(model)) if model == "glm-5.4"));
    });
}

#[test]
fn settings_tab_moves_between_controls() {
    sigil::reactive::Runtime::scope("mage-settings-tab-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.settings_open.set(true);
        app.resize(80, 24);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.settings_sel.peek(), 1);
        let back = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT,
        ))
        .unwrap_or_else(|| panic!("shift+tab did not map"));
        screen
            .frame(&mut host, &mut router, [back], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.settings_sel.peek(), 0);
        assert!(rx.try_recv().is_err(), "settings tab changed the mode");
    });
}
