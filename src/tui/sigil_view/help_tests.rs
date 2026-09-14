use super::*;
use crate::prefs::Hotkey;

#[test]
fn help_keeps_the_reading_position_when_the_terminal_resizes() {
    sigil::reactive::Runtime::scope("help-resize", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(100, 20);
        app.update(Msg::OpenHelp);
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 20);
        let mut frame = screen
            .frame(&mut host, &mut router, [], (100, 20))
            .expect("help")
            .text();
        for _ in 0..100 {
            if frame
                .lines()
                .nth(4)
                .is_some_and(|line| line.contains("ctrl+z/y"))
            {
                break;
            }
            frame = screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::ArrowDown)],
                    (100, 20),
                )
                .expect("read undo reference")
                .text();
        }
        assert!(
            frame
                .lines()
                .nth(4)
                .expect("top reference")
                .contains("ctrl+z/y"),
            "{frame}"
        );
        for width in [40, 100] {
            app.resize(width, 20);
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 20))
                .expect("resize help")
                .text();
            assert!(
                frame
                    .lines()
                    .nth(4)
                    .expect("top reference")
                    .contains("ctrl+z/y"),
                "{frame}"
            );
        }
        app.resize(40, 3);
        let small = screen
            .frame(&mut host, &mut router, [], (40, 3))
            .expect("short help window")
            .text();
        assert!(
            small
                .lines()
                .next()
                .expect("title")
                .contains("Help · esc close"),
            "{small}"
        );
        assert!(small.contains("Enlarge terminal to read help."), "{small}");
        screen
            .frame(
                &mut host,
                &mut router,
                [
                    sigil_tui::key_down(Key::PageDown),
                    sigil_tui::wheel(10, 1, -3.0),
                ],
                (40, 3),
            )
            .expect("navigation while too short");
        app.resize(100, 20);
        let restored = screen
            .frame(&mut host, &mut router, [], (100, 20))
            .expect("enlarge help")
            .text();
        assert!(
            restored
                .lines()
                .nth(4)
                .expect("top reference")
                .contains("ctrl+z/y"),
            "{restored}"
        );
    });
}

#[test]
fn help_filters_and_pages_without_disturbing_the_conversation() {
    for width in [40, 100] {
        for busy in [false, true] {
            sigil::reactive::Runtime::scope("browsable-help", || {
                let (cmd, mut rx) = unbounded_channel();
                let app = App::new(info(), PathBuf::from("."), cmd);
                app.resize(width, 20);
                for index in 0..80 {
                    app.model
                        .borrow_mut()
                        .entries
                        .push(Entry::System(format!("Reading row {index:02}")));
                }
                assert!(
                    app.model
                        .borrow_mut()
                        .prefs
                        .hotkeys
                        .assign(Hotkey::Copy, 'f')
                );
                if busy {
                    app.update(Msg::Busy(Some(Activity::Turn)));
                }
                app.refresh();
                let tree = app.clone();
                let mut host = Host::cells();
                host.mount(move |cx: &mut Cx| cx.add(root(tree)));
                let mut router = EventRouter::new();
                let mut screen = Screen::new(width, 20);
                screen
                    .frame(&mut host, &mut router, [], (width, 20))
                    .expect("chat");
                app.scroll_y.set(15.0 * Host::CELL);
                let before = screen
                    .frame(&mut host, &mut router, [], (width, 20))
                    .expect("reading")
                    .text();
                let position = app.scroll_y.peek();
                router.focus_next(host.cx());
                let events = "/help"
                    .chars()
                    .map(KeyCode::Char)
                    .chain([KeyCode::Enter])
                    .map(|key| {
                        sigil_tui::map_key(&TerminalKeyEvent::new(key, KeyModifiers::NONE))
                            .expect("help command")
                    });
                let opened = screen
                    .frame(&mut host, &mut router, events, (width, 20))
                    .expect("help")
                    .text();
                assert!(opened.contains("Help · keys and commands"), "{opened}");
                assert!(opened.contains("/model"), "{opened}");
                assert!(
                    rx.try_recv().is_err(),
                    "help is available without a worker round trip"
                );
                let paged = screen
                    .frame(
                        &mut host,
                        &mut router,
                        [sigil_tui::key_down(Key::PageDown)],
                        (width, 20),
                    )
                    .expect("page help")
                    .text();
                assert_ne!(paged, opened);
                let wheeled = screen
                    .frame(
                        &mut host,
                        &mut router,
                        [sigil_tui::wheel(10, 8, 3.0)],
                        (width, 20),
                    )
                    .expect("wheel help")
                    .text();
                assert_ne!(wheeled, paged);
                assert_eq!(app.scroll_y.peek(), position);
                let events = "COPY LAST REPLY".chars().map(|key| {
                    sigil_tui::map_key(&TerminalKeyEvent::new(
                        KeyCode::Char(key),
                        KeyModifiers::NONE,
                    ))
                    .expect("filter")
                });
                let filtered = screen
                    .frame(&mut host, &mut router, events, (width, 20))
                    .expect("filter help")
                    .text();
                assert!(filtered.contains("ctrl+f"), "{filtered}");
                assert!(filtered.contains("/copy"), "{filtered}");
                assert!(!filtered.contains("/model"), "{filtered}");
                assert!(!filtered.contains("ctrl+b"), "{filtered}");
                let joined: String = filtered
                    .lines()
                    .skip(4)
                    .take(15)
                    .flat_map(str::split_whitespace)
                    .collect();
                assert!(joined.contains("copythelastreply"), "{filtered}");
                let entered = screen
                    .frame(
                        &mut host,
                        &mut router,
                        [sigil_tui::key_down(Key::Enter)],
                        (width, 20),
                    )
                    .expect("keep filtered help")
                    .text();
                assert_eq!(entered, filtered);
                let newline = sigil_tui::map_key(&TerminalKeyEvent::new(
                    KeyCode::Char('j'),
                    KeyModifiers::CONTROL,
                ))
                .expect("newline shortcut");
                let unchanged = screen
                    .frame(&mut host, &mut router, [newline], (width, 20))
                    .expect("keep filter on newline shortcut")
                    .text();
                assert_eq!(unchanged, filtered);
                let missing = screen
                    .frame(
                        &mut host,
                        &mut router,
                        [sigil_tui::map_key(&TerminalKeyEvent::new(
                            KeyCode::Char('!'),
                            KeyModifiers::NONE,
                        ))
                        .expect("no match")],
                        (width, 20),
                    )
                    .expect("empty help filter")
                    .text();
                assert!(
                    missing.contains("No matches. Edit the filter."),
                    "{missing}"
                );
                let restored = screen
                    .frame(
                        &mut host,
                        &mut router,
                        [sigil_tui::key_down(Key::Escape)],
                        (width, 20),
                    )
                    .expect("close help")
                    .text();
                assert_eq!(restored, before);
                assert_eq!(app.model.borrow().entries.len(), 80);
                assert!(
                    rx.try_recv().is_err(),
                    "closing help does not cancel the active turn"
                );
                app.replace("keep this café draft".to_string());
                app.update(Msg::OpenHelp);
                screen
                    .frame(&mut host, &mut router, [], (width, 20))
                    .expect("requested help");
                let cancel = sigil_tui::map_key(&TerminalKeyEvent::new(
                    KeyCode::Char('c'),
                    KeyModifiers::CONTROL,
                ))
                .expect("close shortcut");
                screen
                    .frame(&mut host, &mut router, [cancel], (width, 20))
                    .expect("close requested help");
                assert!(!app.help.open.peek());
                assert_eq!(app.draft.peek(), "keep this café draft");
                assert!(rx.try_recv().is_err());
            });
        }
    }
}
