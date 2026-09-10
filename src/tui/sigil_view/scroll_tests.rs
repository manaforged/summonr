use super::*;

#[test]
fn scrolling_projection_updates_when_streamed_text_changes() {
    sigil::reactive::Runtime::scope("summoner-streamed-history", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        app.update(Msg::Delta("stream beginning".to_string()));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        let initial = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("first delta")
            .text();
        assert!(initial.contains("stream beginning"));
        app.update(Msg::Delta(" and continuation".to_string()));
        app.refresh();
        let updated = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("next delta")
            .text();
        assert!(
            updated.contains("stream beginning and continuation"),
            "{updated}"
        );
    });
}

#[test]
fn long_history_scroll_keeps_the_visible_text() {
    sigil::reactive::Runtime::scope("summoner-long-scroll", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..2000 {
            app.model.borrow_mut().entries.push(Entry::Assistant {
                text: format!(
                    "Reply {index}\n\n{}",
                    "Text with **emphasis** and `code`. ".repeat(20)
                ),
                done: true,
            });
        }
        app.resize(120, 40);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(120, 40);
        screen
            .frame(&mut host, &mut router, [], (120, 40))
            .expect("initial frame");
        app.scroll_y.set(0.0);
        let mut frames = Vec::new();
        let started = std::time::Instant::now();
        for _ in 0..20 {
            let grid = screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::wheel(10, 5, -1.0)],
                    (120, 40),
                )
                .expect("scroll frame");
            frames.push(grid.text());
        }
        assert!(frames[0].contains("Reply 1"), "{}", frames[0]);
        assert_ne!(frames[0], frames[19]);
        if let Some(path) = std::env::var_os("SUMMON_SCROLL_CAPTURE") {
            std::fs::write(path, serde_json::to_vec(&frames).expect("capture JSON"))
                .expect("write capture");
            eprintln!("20 long-history frames: {:?}", started.elapsed());
        }
    });
}

#[test]
fn the_first_wheel_notch_moves_away_from_the_visible_tail() {
    sigil::reactive::Runtime::scope("summoner-scroll-tail", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        for index in 0..100 {
            app.model.borrow_mut().entries.push(Entry::Assistant {
                text: format!("```rust\nlet value = {index};\n```"),
                done: true,
            });
        }
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        let tail = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("tail")
            .text();
        assert!(tail.contains("let value = 99;"));
        let moved = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::wheel(10, 5, 1.0)],
                (80, 24),
            )
            .expect("first notch")
            .text();
        assert_ne!(
            moved, tail,
            "the first notch must move text, not just an estimated offset"
        );
        assert!(!moved.contains("let value = 99;"));
        let returned = screen
            .frame(
                &mut host,
                &mut router,
                [sigil_tui::wheel(10, 5, -1.0)],
                (80, 24),
            )
            .expect("return to tail")
            .text();
        assert_eq!(returned, tail);
    });
}

#[test]
fn transcript_opens_on_the_newest_output() {
    sigil::reactive::Runtime::scope("mage-transcript-tail-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.refresh();
        let tree = app.clone();
        let Ok(frame) = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 40, 20, &[])
        else {
            panic!("layout failed");
        };

        assert!(
            frame.contains("transcript row 39"),
            "newest row missing:\n{frame}"
        );
        assert!(
            !frame.contains("transcript row 0"),
            "oldest row still visible:\n{frame}"
        );
    });
}

#[test]
fn page_up_scrolls_history_while_the_composer_is_focused() {
    sigil::reactive::Runtime::scope("mage-transcript-page-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.refresh();
        let first = app.clone();
        if let Err(error) = sigil_tui::present(move |cx: &mut Cx| cx.add(root(first)), 40, 20, &[])
        {
            panic!("layout failed: {error}");
        }
        let bottom = app.scroll_y.peek();
        let tree = app.clone();
        let result = sigil_tui::present(
            move |cx: &mut Cx| cx.add(root(tree)),
            40,
            20,
            &[sigil_tui::key_down(Key::PageUp)],
        );
        if let Err(error) = result {
            panic!("layout failed: {error}");
        }

        assert!(app.scroll_y.peek() < bottom);
    });
}

#[test]
fn mouse_wheel_scrolls_transcript_history() {
    sigil::reactive::Runtime::scope("mage-transcript-wheel-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.resize(40, 20);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(40, 20);
        screen
            .frame(&mut host, &mut router, [], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let bottom = app.scroll_y.peek();
        let mut pointer = Point::new(0.0, 0.0);
        let wheel = sigil_tui::map_mouse(
            &MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 10,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            Host::CELL,
            &mut pointer,
        )
        .unwrap_or_else(|| panic!("wheel did not map"));
        screen
            .frame(&mut host, &mut router, [wheel], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));

        assert!(app.scroll_y.peek() < bottom);
    });
}

#[test]
fn repeated_page_up_reaches_wrapped_history_start() {
    sigil::reactive::Runtime::scope("mage-transcript-full-scroll-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        for index in 0..40 {
            app.model.borrow_mut().entries.push(Entry::System(format!(
                "transcript row {index} {}",
                "wrapped text ".repeat(12)
            )));
        }
        app.refresh();
        let tree = app.clone();
        let mut host = sigil_tui::Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = sigil_tui::EventRouter::new();
        let mut screen = sigil_tui::Screen::new(40, 20);
        screen
            .frame(&mut host, &mut router, [], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        router.focus_next(host.cx());
        for _ in 0..20 {
            screen
                .frame(
                    &mut host,
                    &mut router,
                    [sigil_tui::key_down(Key::PageUp)],
                    (40, 20),
                )
                .unwrap_or_else(|error| panic!("layout failed: {error}"));
        }
        let frame = screen
            .frame(&mut host, &mut router, [], (40, 20))
            .unwrap_or_else(|error| panic!("layout failed: {error}"))
            .text();

        assert!(
            frame.contains("transcript row 0"),
            "oldest row missing:\n{frame}"
        );
    });
}
