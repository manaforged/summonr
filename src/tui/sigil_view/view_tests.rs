use super::*;

#[test]
fn slash_menu_routes_arrows_tab_and_escape() {
    sigil::reactive::Runtime::scope("mage-command-menu-keys-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 24);
        app.replace("/".to_string());
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

        let selected = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowDown)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(selected.text().contains("› /settings"));

        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "/settings");

        let hidden = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Escape)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();
        assert!(!hidden.text().contains("commands"));
        assert!(!app.settings_open.peek());
    });
}

#[test]
fn escape_cancels_busy_turn_with_slash_draft() {
    sigil::reactive::Runtime::scope("mage-busy-slash-escape-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 24);
        app.replace("/".to_string());
        app.model.borrow_mut().busy_since = Some(Instant::now());
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
                [sigil_tui::key_down(Key::Escape)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(matches!(rx.try_recv(), Ok(Cmd::Cancel)));
    });
}

#[test]
fn composer_maps_enter_and_newline() {
    sigil::reactive::Runtime::scope("mage-composer-key-parity-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
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

        app.replace("line one".to_string());
        let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ))
        .unwrap_or_else(|| panic!("ctrl+j did not map"));
        screen
            .frame(&mut host, &mut router, [newline], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "line one\n");

        app.replace("send now".to_string());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Submit(_))));
        assert!(app.draft.peek().is_empty());
    });
}

#[test]
fn newline_closes_suggestions_before_enter() {
    sigil::reactive::Runtime::scope("mage-slash-newline-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(80, 24);
        app.replace("/help".to_string());
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
        let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ))
        .unwrap_or_else(|| panic!("ctrl+j did not map"));

        screen
            .frame(&mut host, &mut router, [newline], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Enter)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(app.draft.peek().is_empty());
    });
}

#[test]
fn composer_hotkeys_keep_completion_queue_settings_and_editor_distinct() {
    sigil::reactive::Runtime::scope("mage-composer-hotkey-test", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
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

        app.model.borrow_mut().busy_since = Some(Instant::now());
        app.replace("queue next".to_string());
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::Tab)],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::Queue(input)) if input.text == "queue next"));

        app.model.borrow_mut().busy_since = None;
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::map_key(&TerminalKeyEvent::new(
                    KeyCode::BackTab,
                    KeyModifiers::SHIFT,
                ))
                .unwrap_or_else(|| panic!("shift+tab did not map"))],
                (80, 24),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(matches!(rx.try_recv(), Ok(Cmd::SetMode(true))));

        let ctrl = |ch| {
            sigil_tui::map_key(&TerminalKeyEvent::new(
                KeyCode::Char(ch),
                KeyModifiers::CONTROL,
            ))
            .unwrap_or_else(|| panic!("ctrl+{ch} did not map"))
        };
        screen
            .frame(&mut host, &mut router, [ctrl('o')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.settings_open.peek());
        screen
            .frame(&mut host, &mut router, [ctrl('o')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(!app.settings_open.peek());

        screen
            .frame(&mut host, &mut router, [ctrl('g')], (80, 24))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert!(app.editor_requested.peek());
    });
}

#[test]
fn visible_caret_tracks_editor_navigation() {
    sigil::reactive::Runtime::scope("mage-visible-caret-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.resize(40, 12);
        app.replace("hello".to_string());
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(40, 12);
        screen
            .frame(&mut host, &mut router, [], (40, 12))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());
        let frame = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::key_down(Key::ArrowLeft)],
                (40, 12),
            )
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .clone();

        assert_eq!(app.caret.peek(), 4);
        assert!(
            frame.text().contains("hell▌o"),
            "caret did not move:\n{}",
            frame.text()
        );

        let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
            KeyCode::Char('j'),
            KeyModifiers::CONTROL,
        ))
        .unwrap_or_else(|| panic!("ctrl+j did not map"));
        screen
            .frame(&mut host, &mut router, [newline], (40, 12))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        assert_eq!(app.draft.peek(), "hell\no");
        assert_eq!(app.caret.peek(), 5);
    });
}
